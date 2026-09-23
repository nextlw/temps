// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Wire contract between Temps and a build runner.
//!
//! # The one idea in this crate
//!
//! A build runner does not know *why* it is building. It receives a context, a
//! recipe, a target, a budget and an identity; it returns logs, a result
//! envelope and artifacts. Whether the caller was a deployment pipeline, a
//! release job for a desktop binary, or a developer pushing a branch is not
//! information this protocol carries, because none of it changes how the build
//! runs.
//!
//! That is the whole point. Today `temps-deployer` registers exactly one
//! `ImageBuilder` — the local Docker daemon — and every build lands on the
//! control plane. With four developers pushing to four branches, that is not a
//! contention problem to be throttled; it is a concurrency problem, and
//! concurrency is answered by distribution. Distribution needs a contract that
//! does not assume the builder is the machine holding the database.
//!
//! # Design constraints
//!
//! **A build does not return an image; it returns an envelope.** A runner on
//! another machine — or on Windows, or on macOS — has no way to hand an image
//! to the control plane's local daemon. Five jobs downstream of the build read
//! the image off that daemon today. Three of them only need the image *config*,
//! which is why [`BuildResultEnvelope::config`] is authoritative: with it,
//! `inspect_image` needs no local image. The remaining two need bytes, which is
//! why the work that needs bytes is requested up front through
//! [`OutputRequest`] and happens where the image already is.
//!
//! **Nothing here is OCI-shaped by default.** [`BuildResultEnvelope::digest`]
//! and [`BuildResultEnvelope::config`] are optional because a desktop binary has
//! neither. The deployment path narrows this in its own adapter and fails loudly
//! when an image was expected and not produced. Widening `ImageBuilder` instead
//! would push "maybe there is no image" into every consumer, which is exactly
//! the property the seam exists to protect.
//!
//! **Workspace isolation and cache scope are different things.** A build gets a
//! private workspace so one branch never sees another's files; it shares a
//! [`CacheScope`] with the rest of its project so the second push of the morning
//! does not rebuild dependency layers from zero. Conflating the two gives you
//! either leakage or a permanently cold cache.
//!
//! **Two axes, deliberately separate.** [`BuildTarget`] answers *can this
//! machine run it* — os, architecture, capabilities. The environment on
//! [`Requester`] answers *what may this build reach, and where does the result
//! land*. Collapsing them is how a development build ends up holding a
//! production credential.
//!
//! **The environment is a credential boundary, and the spawner owns it.** A
//! runner never selects its own credentials: the host-side spawner decides what
//! goes into the child's environment, per environment, from an allowlist.
//! Nothing in this protocol lets a build ask for more, which is the property
//! that makes the boundary enforceable rather than advisory. A build is running
//! third-party code by definition — a dependency tree's install scripts — so
//! the credential it is not given is the only one it cannot leak.
//!
//! **Priority comes from the environment, not from the requester.** A developer
//! cannot promote their own build ahead of a production release by asking
//! nicely; see [`Priority`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Schema tag every envelope carries, so a host that parses a runner's stdout
/// can tell "this is a result" from "this is whatever the build printed last".
///
/// A build's stdout is shared with the program being built, which prints
/// whatever it likes. Without a tag, a project whose test suite happens to emit
/// JSON could be read as a build result. Bump the version when the shape
/// changes incompatibly; a peer that does not recognise it must refuse rather
/// than guess.
pub const ENVELOPE_SCHEMA: &str = "temps.build/v1";

/// A unit of work handed to a runner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildRequest {
    /// Stable id, so a runner that reconnects mid-build can be reconciled
    /// instead of duplicating work.
    pub build_id: uuid::Uuid,

    /// Where the source comes from.
    pub context: BuildContext,

    /// How to turn that source into something.
    pub recipe: BuildRecipe,

    /// Which runners may accept this build.
    pub target: BuildTarget,

    /// Ceiling and queue position.
    pub budget: BuildBudget,

    /// Who asked. Carried for quota and audit, never for authorization: a
    /// runner does not decide what a requester may do.
    pub requester: Requester,

    /// What the caller wants back. Declared up front so extraction and scanning
    /// can run on the machine that already holds the image.
    pub outputs: Vec<OutputRequest>,

    /// Which layer cache this build may read and write.
    pub cache: CacheScope,
}

/// Where a runner fetches the source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuildContext {
    /// A commit in a repository the runner can reach.
    Git {
        url: String,
        /// Branch or tag as written by the trigger, kept for logs and cache
        /// keys. The commit is what is actually built.
        reference: String,
        commit: String,
    },
    /// An archive already uploaded to Temps. This is the path a working tree
    /// takes: a developer's uncommitted changes are not a commit, and pretending
    /// otherwise would mean inventing one.
    Archive {
        upload_id: uuid::Uuid,
        /// Content digest of the archive, so a runner can prove it fetched what
        /// the control plane meant and a cache key can be derived from it.
        digest: String,
    },
}

/// How the source becomes an output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuildRecipe {
    /// A Dockerfile in the context.
    Dockerfile {
        /// Relative to the context root.
        path: String,
        /// Relative to the context root; defaults to the context root itself.
        build_dir: Option<String>,
        build_args: BTreeMap<String, String>,
    },
    /// One of the framework presets already in the tree, which produce a
    /// Dockerfile of their own.
    Preset {
        name: String,
        build_args: BTreeMap<String, String>,
    },
    /// A command run in the runner's own environment, for outputs that are not
    /// container images — a desktop binary, an installer, a signed bundle.
    Native {
        command: Vec<String>,
        env: BTreeMap<String, String>,
        /// Relative to the context root.
        working_dir: Option<String>,
    },
}

/// The set of runners that may accept a build.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildTarget {
    pub os: Os,
    pub arch: Arch,
    /// Everything the recipe needs the runner to already have. A runner
    /// advertises what it can do; the router matches, it does not install.
    /// Examples: `buildkit`, `buildkit-rootless`, `docker-socket`, `codesign`.
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Linux,
    Windows,
    MacOs,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    Amd64,
    Arm64,
}

/// Ceiling and queue position for one build.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildBudget {
    /// Hard stop. A build that has not finished is killed, not throttled: a
    /// developer waiting on a hung build wants to be told, not queued behind it.
    pub timeout_secs: u32,
    /// Cores, in microcores, matching `DeploymentConfig`'s existing unit
    /// (1_000_000 = one core) rather than inventing a second one.
    pub cpu_limit_micros: Option<u64>,
    pub memory_limit_bytes: Option<u64>,
    pub priority: Priority,
}

/// Queue position, derived from the environment the build is for.
///
/// Deliberately not requester-supplied. With four developers pushing branches
/// all day, a production release must not wait behind six of their builds, and
/// no developer should be able to change that by editing a payload.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Branch pushes. The most builds, the least urgency per build.
    Development = 0,
    /// Optional. Not every project has a stage between development and
    /// production, and the ladder must not require one.
    Staging = 1,
    /// Tags and production promotions.
    Production = 2,
}

/// Who asked for the build. Quota and audit only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Requester {
    pub user_id: Option<uuid::Uuid>,
    pub project_id: uuid::Uuid,
    /// Which environment this build is for. Carries more weight than an
    /// identifier: it selects the credential set the spawner injects and the
    /// target the result is delivered to. `None` only for a build whose result
    /// goes back to the requester and reaches no environment at all.
    pub environment_id: Option<i32>,
}

/// What the caller wants back.
///
/// Requested before the build starts, because the point of asking is to keep
/// the work next to the image instead of shipping the image to the work.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputRequest {
    /// Push the image and report its digest, platforms and config.
    Image { registry_ref: String },
    /// Collect files from the built filesystem. Used by the static deploy path
    /// and by desktop builds, which have no image at all.
    Files { globs: Vec<String> },
    /// Run the vulnerability scan on the runner. Moving this is most of the
    /// second multi-minute CPU burst off the control plane.
    Scan,
}

/// What a runner reports when the build succeeded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildResultEnvelope {
    /// Always [`ENVELOPE_SCHEMA`]. Checked on parse, never trusted from the
    /// wire without comparison.
    pub schema: String,

    pub build_id: uuid::Uuid,

    /// Present when the build produced a container image. `None` for a native
    /// build, which is why callers that require an image must say so.
    pub digest: Option<String>,

    pub platforms: Vec<String>,

    /// Authoritative image config, so a consumer needs no local image to read
    /// `WORKDIR`, `ExposedPorts`, `User`, `Env` or `Entrypoint`.
    pub config: Option<serde_json::Value>,

    /// Everything the plan asked for under [`OutputRequest::Files`].
    pub artifacts: Vec<ArtifactRef>,

    /// Present when the plan asked for a scan.
    pub scan: Option<serde_json::Value>,

    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: chrono::DateTime<chrono::Utc>,
}

impl BuildResultEnvelope {
    /// Whether this envelope claims the schema this build understands.
    #[must_use]
    pub fn schema_matches(&self) -> bool {
        self.schema == ENVELOPE_SCHEMA
    }
}

/// A file the runner uploaded, addressed by content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRef {
    /// Path relative to the collection root, as the glob matched it.
    pub path: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub digest: String,
}

/// Which layer cache a build may read and write.
///
/// Scoped to the project, not to the build: isolation belongs to the workspace.
/// A build sees no other build's files and still reuses its project's
/// dependency layers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheScope {
    pub project_id: uuid::Uuid,
    /// Set when a build must not read the shared cache — a release that has to
    /// be reproducible from nothing. Writing is still allowed.
    pub read: bool,
}

/// Why a build did not produce an envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuildFailure {
    #[error("no runner matched target {target:?}")]
    NoRunnerMatched { target: BuildTarget },
    #[error("build exceeded its budget of {timeout_secs}s")]
    BudgetExceeded { timeout_secs: u32 },
    #[error("recipe failed with exit status {status}")]
    RecipeFailed { status: i32 },
    #[error("context could not be fetched: {reason}")]
    ContextUnavailable { reason: String },
    #[error("runner disconnected before reporting a result")]
    RunnerLost,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request with every field populated, so a serialization test sees the
    /// whole surface rather than the parts that happen to be easy to build.
    fn full_request() -> BuildRequest {
        BuildRequest {
            build_id: uuid::Uuid::nil(),
            context: BuildContext::Git {
                url: "https://example.invalid/repo.git".to_string(),
                reference: "refs/heads/feature".to_string(),
                commit: "0000000000000000000000000000000000000000".to_string(),
            },
            recipe: BuildRecipe::Dockerfile {
                path: "Dockerfile".to_string(),
                build_dir: Some("backend".to_string()),
                build_args: BTreeMap::from([("VERSION".to_string(), "1".to_string())]),
            },
            target: BuildTarget {
                os: Os::Linux,
                arch: Arch::Amd64,
                capabilities: vec!["buildkit".to_string()],
            },
            budget: BuildBudget {
                timeout_secs: 900,
                cpu_limit_micros: Some(4_000_000),
                memory_limit_bytes: Some(8 * 1024 * 1024 * 1024),
                priority: Priority::Development,
            },
            requester: Requester {
                user_id: Some(uuid::Uuid::nil()),
                project_id: uuid::Uuid::nil(),
                environment_id: Some(1),
            },
            outputs: vec![
                OutputRequest::Image {
                    registry_ref: "registry.invalid/app:sha".to_string(),
                },
                OutputRequest::Scan,
            ],
            cache: CacheScope {
                project_id: uuid::Uuid::nil(),
                read: true,
            },
        }
    }

    /// The control plane and a runner are deployed separately and can drift:
    /// a runner on a build machine, and later on a Windows or macOS host, runs
    /// whatever version it last received. A field renamed on one side and not
    /// the other either fails loudly — the good case — or deserializes into a
    /// default and builds something nobody asked for.
    ///
    /// This test pins the wire names. It fails on any rename, on any change to
    /// a `#[serde(tag)]` discriminator, and on any field added without thought
    /// about the older peer. If runner and control plane are ever guaranteed to
    /// ship together, this test is ceremony and should be deleted rather than
    /// weakened.
    #[test]
    fn the_wire_names_are_pinned_so_a_lagging_runner_fails_loudly() {
        let json = serde_json::to_value(full_request()).expect("a request serializes");

        let top_level: Vec<&str> = json
            .as_object()
            .expect("a request is a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            top_level,
            vec![
                "budget", "build_id", "cache", "context", "outputs", "recipe", "requester",
                "target",
            ],
            "the top-level field names of BuildRequest are wire surface; \
             renaming one breaks every peer that was not redeployed with it"
        );

        assert_eq!(
            json["context"]["kind"], "git",
            "BuildContext::Git is tagged `git` on the wire"
        );
        assert_eq!(
            json["recipe"]["kind"], "dockerfile",
            "BuildRecipe::Dockerfile is tagged `dockerfile` on the wire"
        );
        assert_eq!(
            json["outputs"][0]["kind"], "image",
            "OutputRequest::Image is tagged `image` on the wire"
        );
        assert_eq!(
            json["outputs"][1]["kind"], "scan",
            "OutputRequest::Scan is tagged `scan` on the wire"
        );
        assert_eq!(
            json["target"]["os"], "linux",
            "Os::Linux is `linux` on the wire, not `Linux`"
        );
        assert_eq!(
            json["budget"]["priority"], "development",
            "Priority::Development is `development` on the wire"
        );
    }

    /// A round trip is the property a runner actually depends on: what it
    /// receives is what the control plane meant. Serializing without checking
    /// the way back would let an asymmetric `#[serde]` attribute pass.
    #[test]
    fn a_request_survives_a_round_trip_unchanged() {
        let original = full_request();
        let encoded = serde_json::to_string(&original).expect("a request serializes");
        let decoded: BuildRequest =
            serde_json::from_str(&encoded).expect("a serialized request deserializes");
        assert_eq!(
            decoded, original,
            "a request must survive the wire unchanged; an asymmetric serde \
             attribute shows up here and nowhere else"
        );
    }

    /// Queue position is a discriminant, and a discriminant is reorderable by
    /// accident. If someone swaps two variants, nothing fails to compile and
    /// nothing fails at runtime — production releases simply start waiting
    /// behind branch builds, and the symptom looks like a slow machine.
    #[test]
    fn production_outranks_staging_outranks_development() {
        assert!(
            Priority::Production > Priority::Staging,
            "a production release must never queue behind a staging build"
        );
        assert!(
            Priority::Staging > Priority::Development,
            "a staging build must never queue behind a branch build"
        );

        let mut queue = vec![
            Priority::Development,
            Priority::Production,
            Priority::Development,
            Priority::Staging,
        ];
        queue.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(
            queue,
            vec![
                Priority::Production,
                Priority::Staging,
                Priority::Development,
                Priority::Development,
            ],
            "sorting descending by priority is how the queue is drained; \
             this is the ordering the scheduler will rely on"
        );
    }

    /// A desktop binary has no image digest and no image config. The envelope
    /// has to be able to say so, otherwise the native build path would need a
    /// second envelope type and every consumer would grow a branch.
    ///
    /// The deployment path narrows this in its own adapter: it is the adapter's
    /// job to fail loudly when an image was expected and none was produced.
    #[test]
    fn an_envelope_without_an_image_is_representable() {
        let native = BuildResultEnvelope {
            schema: ENVELOPE_SCHEMA.to_string(),
            build_id: uuid::Uuid::nil(),
            digest: None,
            platforms: vec!["windows/amd64".to_string()],
            config: None,
            artifacts: vec![ArtifactRef {
                path: "dist/installer.exe".to_string(),
                media_type: "application/vnd.microsoft.portable-executable".to_string(),
                size_bytes: 42,
                digest: "sha256:0".to_string(),
            }],
            scan: None,
            started_at: chrono::DateTime::UNIX_EPOCH,
            finished_at: chrono::DateTime::UNIX_EPOCH,
        };

        let round_tripped: BuildResultEnvelope =
            serde_json::from_str(&serde_json::to_string(&native).expect("an envelope serializes"))
                .expect("an envelope deserializes");

        assert_eq!(round_tripped, native);
        assert!(
            round_tripped.digest.is_none() && round_tripped.config.is_none(),
            "a native build reports no image; a contract that cannot say this \
             forces a second envelope type on the desktop path"
        );
        assert_eq!(
            round_tripped.artifacts.len(),
            1,
            "the artifact is the whole result of a native build, not a side effect"
        );
    }

    /// A build's stdout belongs to the program being built as much as to the
    /// runner. Anything on it that is not tagged must be treated as output,
    /// not as a result — otherwise a test suite that prints JSON could be
    /// mistaken for a successful build.
    #[test]
    fn an_envelope_is_only_a_result_when_it_carries_the_schema_tag() {
        let mut envelope = BuildResultEnvelope {
            schema: ENVELOPE_SCHEMA.to_string(),
            build_id: uuid::Uuid::nil(),
            digest: None,
            platforms: vec![],
            config: None,
            artifacts: vec![],
            scan: None,
            started_at: chrono::DateTime::UNIX_EPOCH,
            finished_at: chrono::DateTime::UNIX_EPOCH,
        };
        assert!(envelope.schema_matches(), "the current schema is accepted");

        envelope.schema = "temps.build/v2".to_string();
        assert!(
            !envelope.schema_matches(),
            "a future schema must be refused, not guessed at: a peer that \
             does not recognise the shape cannot know what it is missing"
        );

        envelope.schema = String::new();
        assert!(
            !envelope.schema_matches(),
            "untagged output is output, never a result"
        );
    }

    /// The environment decides which credentials the host-side spawner injects.
    /// A runner never chooses. This test asserts the shape that makes that
    /// enforceable: there is nowhere in a request for a build to ask for a
    /// credential, so a compromised or simply buggy runner cannot request one.
    ///
    /// What this does **not** prove: that the spawner's allowlist is correct.
    /// That lives in the executor and is tested there, against a real child
    /// process. This test only keeps the door from being added back.
    #[test]
    fn a_request_carries_no_credential_field() {
        let json = serde_json::to_string(&full_request()).expect("a request serializes");
        let lowered = json.to_lowercase();

        for forbidden in [
            "secret",
            "token",
            "password",
            "credential",
            "private_key",
            "ssh_auth_sock",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "a serialized BuildRequest must not contain `{forbidden}`: \
                 credentials are injected by the host-side spawner per \
                 environment, never selected by the build"
            );
        }
    }
}
