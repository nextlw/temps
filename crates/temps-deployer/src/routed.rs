// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Routes a build to where it should run, without the pipeline noticing.
//!
//! # Why this is an adapter and not a new job type
//!
//! Five jobs downstream of a build read its output, and the planner adds them
//! on conditions that have nothing to do with where the build ran. A second
//! job type for hosted builds would fork that graph and duplicate every one of
//! those consumers, turning "a hosted build behaves like a local one" into a
//! promise maintained by hand. Keeping [`ImageBuilder`] as the seam makes it a
//! property of the type instead: `WorkflowPlanner` and `BuildImageJob` do not
//! change, and the only edit at the call site is which builder is handed over.
//!
//! # Where "maybe there is no image" is resolved
//!
//! The build contract is deliberately not OCI-shaped: a desktop binary has no
//! digest and no image config, so [`BuildResultEnvelope`] makes both optional.
//! The deployment path cannot live with that ambiguity — it deploys images.
//! This adapter is where the two meet, and it fails loudly rather than
//! inventing a digest. That is the whole reason the ambiguity was pushed here
//! instead of into every consumer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use temps_build_executor::{run_build, BuildInvocation};
use temps_build_protocol::BuildResultEnvelope;

use crate::{
    BuildRequest, BuildRequestWithCallback, BuildResult, BuilderError, ImageBuilder, ImageImportStream,
    ImageInfo,
};

/// Everything a hosted build needs that the request is not allowed to choose.
pub struct HostedBuildPlan {
    pub program: PathBuf,
    pub working_dir: PathBuf,
    pub path: String,
    pub home: PathBuf,
    /// What this environment grants this child, by name. Empty is the
    /// intended value for a build step.
    pub credentials: BTreeMap<String, String>,
    pub wall_timeout: Duration,
    pub request: temps_build_protocol::BuildRequest,
}

/// Where a build runs.
pub enum BuildPlacement {
    /// The local daemon — the behaviour every build has today.
    Local,
    /// A host-side executor.
    Hosted(Box<HostedBuildPlan>),
}

/// Decides placement. Implemented over project and environment configuration;
/// kept as a trait so the decision is testable without a database.
pub trait BuildPolicy: Send + Sync {
    fn placement_for(&self, request: &BuildRequest) -> BuildPlacement;
}

/// A policy that keeps every build where it is today.
///
/// Exists so that wiring this type into the pipeline is not the same change as
/// moving any build: the switch and the behaviour land separately, and a
/// rollback of one is not a rollback of the other.
pub struct AlwaysLocal;

impl BuildPolicy for AlwaysLocal {
    fn placement_for(&self, _request: &BuildRequest) -> BuildPlacement {
        BuildPlacement::Local
    }
}

/// How many hosted builds' facts are remembered at once.
///
/// Bounded because the key is an image name and image names are minted per
/// deployment: an unbounded map here is a slow leak on a machine that is
/// already the busiest one in the cluster. A build's facts are read by the
/// jobs immediately downstream of it, so the useful lifetime is one
/// deployment; this holds far more than that and still cannot grow.
const REMEMBERED_HOSTED_BUILDS: usize = 64;

/// Placement resolved from project and environment configuration.
///
/// Built once per deployment by the caller that already holds the resolved
/// `DeploymentConfig`, because the inheritance rule — environment overrides
/// project, `None` inherits — belongs where the two configs are in hand, not
/// here. What this type does is turn a resolved answer into a plan.
pub struct ConfiguredBuildPolicy {
    /// The build program, when one was configured. `None` keeps every build
    /// on the local daemon, which is what every existing row means.
    program: Option<PathBuf>,
    project_id: i32,
    environment_id: Option<i32>,
    priority: temps_build_protocol::Priority,
    wall_timeout: Duration,
    path: String,
    home: PathBuf,
}

impl ConfiguredBuildPolicy {
    pub fn new(
        program: Option<PathBuf>,
        project_id: i32,
        environment_id: Option<i32>,
        priority: temps_build_protocol::Priority,
        wall_timeout: Duration,
        path: String,
        home: PathBuf,
    ) -> Self {
        Self {
            program,
            project_id,
            environment_id,
            priority,
            wall_timeout,
            path,
            home,
        }
    }
}

impl BuildPolicy for ConfiguredBuildPolicy {
    fn placement_for(&self, request: &BuildRequest) -> BuildPlacement {
        let Some(program) = self.program.clone() else {
            return BuildPlacement::Local;
        };

        let recipe = temps_build_protocol::BuildRecipe::Dockerfile {
            path: request
                .dockerfile_path
                .as_ref()
                .map_or_else(|| "Dockerfile".to_string(), |p| p.display().to_string()),
            build_dir: None,
            build_args: request
                .build_args
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        };

        // The platform the deployment path asked for, parsed back into the
        // protocol's own vocabulary. An unparseable or absent platform means
        // this host's own, which is what a local build would have produced.
        let (os, arch) = request
            .platform
            .as_deref()
            .and_then(parse_platform)
            .unwrap_or((temps_build_protocol::Os::Linux, temps_build_protocol::Arch::Amd64));

        BuildPlacement::Hosted(Box::new(HostedBuildPlan {
            program,
            working_dir: request.context_path.clone(),
            path: self.path.clone(),
            home: self.home.clone(),
            // Empty on purpose: the build step runs a dependency tree's
            // install scripts. Cloning and pushing are separate spawns with
            // their own, narrower environments.
            credentials: BTreeMap::new(),
            wall_timeout: self.wall_timeout,
            request: temps_build_protocol::BuildRequest {
                build_id: uuid::Uuid::new_v4(),
                context: temps_build_protocol::BuildContext::Archive {
                    upload_id: uuid::Uuid::nil(),
                    digest: String::new(),
                },
                recipe,
                target: temps_build_protocol::BuildTarget {
                    os,
                    arch,
                    capabilities: vec!["buildkit".to_string()],
                },
                budget: temps_build_protocol::BuildBudget {
                    timeout_secs: self.wall_timeout.as_secs().min(u64::from(u32::MAX)) as u32,
                    cpu_limit_micros: None,
                    memory_limit_bytes: None,
                    priority: self.priority,
                },
                requester: temps_build_protocol::Requester {
                    user_id: None,
                    project_id: self.project_id,
                    environment_id: self.environment_id,
                },
                outputs: vec![temps_build_protocol::OutputRequest::Image {
                    registry_ref: request.image_name.clone(),
                }],
                cache: temps_build_protocol::CacheScope {
                    project_id: self.project_id,
                    read: true,
                },
            },
        }))
    }
}

/// Parse `"linux/amd64"` into the protocol's own vocabulary.
///
/// Returns `None` for anything this protocol cannot name, rather than
/// approximating. A build sent to the wrong architecture fails late and
/// confusingly; refusing to guess keeps it on this host's own platform, which
/// is what a local build would have done anyway.
fn parse_platform(
    platform: &str,
) -> Option<(temps_build_protocol::Os, temps_build_protocol::Arch)> {
    let (os, arch) = platform.split_once('/')?;
    let os = match os {
        "linux" => temps_build_protocol::Os::Linux,
        "windows" => temps_build_protocol::Os::Windows,
        "darwin" | "macos" => temps_build_protocol::Os::MacOs,
        _ => return None,
    };
    let arch = match arch {
        "amd64" | "x86_64" => temps_build_protocol::Arch::Amd64,
        "arm64" | "aarch64" => temps_build_protocol::Arch::Arm64,
        _ => return None,
    };
    Some((os, arch))
}

pub struct RoutedImageBuilder {
    policy: Arc<dyn BuildPolicy>,
    local: Arc<dyn ImageBuilder>,
    /// Facts for images this builder produced elsewhere.
    ///
    /// The envelope dies at the trait boundary — `BuildResult` carries an id,
    /// a name, a size and a duration, and nothing else. Without this, a hosted
    /// build would answer `inspect_image` from a daemon that never saw the
    /// image. Remembering the facts here is what lets every consumer that goes
    /// through the trait keep working unchanged, which was the whole argument
    /// for keeping `ImageBuilder` as the seam.
    hosted: std::sync::Mutex<std::collections::VecDeque<(String, ImageFacts, u64)>>,
}

impl RoutedImageBuilder {
    pub fn new(policy: Arc<dyn BuildPolicy>, local: Arc<dyn ImageBuilder>) -> Self {
        Self {
            policy,
            local,
            hosted: std::sync::Mutex::new(std::collections::VecDeque::new()),
        }
    }

    /// Record what a hosted build reported about the image it produced.
    ///
    /// Oldest entries are dropped first. A lock poisoned by a panicking
    /// sibling is recovered rather than propagated: losing a memo makes an
    /// `inspect_image` fall back to the daemon, which is wrong but recoverable,
    /// while a panic here would take down a deployment for a cache miss.
    fn remember(&self, image_name: &str, facts: ImageFacts, size_bytes: u64) {
        let mut hosted = match self.hosted.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        hosted.retain(|(name, _, _)| name != image_name);
        if hosted.len() >= REMEMBERED_HOSTED_BUILDS {
            hosted.pop_front();
        }
        hosted.push_back((image_name.to_string(), facts, size_bytes));
    }

    /// What a hosted build reported about this image, if this builder produced
    /// it and still remembers.
    fn recall(&self, image_name: &str) -> Option<(ImageFacts, u64)> {
        let hosted = match self.hosted.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        hosted
            .iter()
            .find(|(name, _, _)| name == image_name)
            .map(|(_, facts, size)| (facts.clone(), *size))
    }

    async fn build_hosted(
        &self,
        plan: HostedBuildPlan,
        image_name: &str,
    ) -> Result<BuildResult, BuilderError> {
        let envelope = run_build(BuildInvocation {
            request: &plan.request,
            program: plan.program,
            working_dir: plan.working_dir,
            path: plan.path,
            home: plan.home,
            credentials: plan.credentials,
            wall_timeout: plan.wall_timeout,
        })
        .await
        .map_err(|e| BuilderError::BuildFailed(e.to_string()))?;

        if let Some(config) = &envelope.config {
            self.remember(
                image_name,
                image_facts_from_config(config),
                envelope.size_bytes.unwrap_or(0),
            );
        }

        narrow_to_image(envelope, image_name)
    }
}

/// Turn an envelope into the image result the deployment path requires.
///
/// Separate from the trait impl so the narrowing rule — the one place where a
/// build that produced no image meets a path that needs one — is testable on
/// its own.
pub fn narrow_to_image(
    envelope: BuildResultEnvelope,
    image_name: &str,
) -> Result<BuildResult, BuilderError> {
    let Some(image_id) = envelope.digest else {
        return Err(BuilderError::BuildFailed(format!(
            "build {} produced no image, so it cannot be deployed as one: it reported {} artifact(s) \
             and no digest. A recipe that produces a binary belongs to the artifact path, not this one",
            envelope.build_id,
            envelope.artifacts.len()
        )));
    };

    let duration_ms = (envelope.finished_at - envelope.started_at)
        .num_milliseconds()
        .max(0);

    Ok(BuildResult {
        image_id,
        image_name: image_name.to_string(),
        // Only the machine that built it can weigh it; asking the control
        // plane would mean pulling the image back to measure it.
        size_bytes: envelope.size_bytes.unwrap_or(0),
        build_duration_ms: duration_ms.unsigned_abs(),
    })
}

#[async_trait]
impl ImageBuilder for RoutedImageBuilder {
    async fn build_image(&self, request: BuildRequest) -> Result<BuildResult, BuilderError> {
        match self.policy.placement_for(&request) {
            BuildPlacement::Local => self.local.build_image(request).await,
            BuildPlacement::Hosted(plan) => {
                let image_name = request.image_name.clone();
                self.build_hosted(*plan, &image_name).await
            }
        }
    }

    /// Hosted builds do not feed the callback yet.
    ///
    /// The callback streams lines as the local daemon produces them. A hosted
    /// build's output arrives through the executor, and wiring it into this
    /// callback is a separate change; until then a hosted build with a
    /// callback runs and reports, it simply does not narrate. Stated here
    /// rather than silently dropping the callback, because a deployment view
    /// that shows nothing looks like a hung build.
    async fn build_image_with_callback(
        &self,
        request: BuildRequestWithCallback,
    ) -> Result<BuildResult, BuilderError> {
        match self.policy.placement_for(&request.request) {
            BuildPlacement::Local => self.local.build_image_with_callback(request).await,
            BuildPlacement::Hosted(plan) => {
                let image_name = request.request.image_name.clone();
                self.build_hosted(*plan, &image_name).await
            }
        }
    }

    /// Importing is always local: it puts an image into the daemon this
    /// control plane deploys from.
    async fn import_image(&self, image_path: PathBuf, tag: &str) -> Result<String, BuilderError> {
        self.local.import_image(image_path, tag).await
    }

    async fn import_image_stream(
        &self,
        stream: ImageImportStream,
        tag: &str,
    ) -> Result<String, BuilderError> {
        self.local.import_image_stream(stream, tag).await
    }

    // ── The six below are the coupling this whole design exists to break ──
    //
    // They read an image off the local daemon. `inspect_image` has three
    // callers downstream of a build and `extract_from_image` has two, and
    // none of them care where the build ran — which is exactly the problem:
    // an image built elsewhere is not on this daemon, so these answer wrongly
    // rather than loudly.
    //
    // They delegate locally because that is correct for every build that runs
    // locally, which today is all of them. Rewiring the three `inspect_image`
    // callers to read [`BuildResultEnvelope::config`] and moving the two
    // extraction callers to where the image already is, is the change that
    // makes hosted placement usable. Until it lands, a policy that returns
    // `Hosted` produces a build whose image nothing downstream can find.
    //
    // That is why `AlwaysLocal` exists and why the pipeline is not switched
    // over in the same change that introduced this type.

    async fn save_image(&self, image_name: &str, output_path: &Path) -> Result<(), BuilderError> {
        self.local.save_image(image_name, output_path).await
    }

    async fn extract_from_image(
        &self,
        image_name: &str,
        source_path: &str,
        destination_path: &Path,
    ) -> Result<(), BuilderError> {
        self.local
            .extract_from_image(image_name, source_path, destination_path)
            .await
    }

    async fn list_images(&self) -> Result<Vec<String>, BuilderError> {
        self.local.list_images().await
    }

    async fn remove_image(&self, image_name: &str) -> Result<(), BuilderError> {
        self.local.remove_image(image_name).await
    }

    /// Answers from the hosted build's own report when this builder produced
    /// the image elsewhere, and from the daemon otherwise.
    ///
    /// This is the method that makes hosted placement possible without
    /// touching a single consumer: the three callers downstream of a build ask
    /// the same question of the same trait and get an answer that is true,
    /// rather than an answer from a daemon that never saw the image.
    async fn inspect_image(&self, image_name: &str) -> Result<ImageInfo, BuilderError> {
        let Some((facts, size_bytes)) = self.recall(image_name) else {
            return self.local.inspect_image(image_name).await;
        };

        Ok(ImageInfo {
            id: image_name.to_string(),
            architecture: facts.architecture.clone().unwrap_or_default(),
            os: facts.os.clone().unwrap_or_default(),
            platform: facts.platform().unwrap_or_default(),
            size_bytes,
            tags: vec![image_name.to_string()],
            // The runner does not report a creation timestamp and this host
            // did not witness one. `None` says so; a value invented here would
            // be indistinguishable from a real one.
            created: None,
            working_dir: facts.working_dir,
        })
    }

    fn get_native_platform(&self) -> String {
        self.local.get_native_platform()
    }

    fn discovered_platform(&self) -> Option<String> {
        self.local.discovered_platform()
    }
}

/// What the pipeline actually needs to know about a built image.
///
/// Every consumer downstream of a build reads some subset of this, and today
/// they all read it off the local daemon. That is the coupling that keeps a
/// build from running anywhere else — not the build itself. Naming the subset
/// is what lets the same facts come from an envelope instead.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImageFacts {
    /// `WORKDIR`. Two consumers extract files relative to it, and getting it
    /// wrong means extracting from the wrong place rather than failing.
    pub working_dir: Option<String>,
    /// Ports from `EXPOSE`, already parsed. The deploy path publishes these.
    pub exposed_ports: Vec<u16>,
    pub architecture: Option<String>,
    pub os: Option<String>,
}

impl ImageFacts {
    /// Platform as the deploy path writes it, when both halves are known.
    #[must_use]
    pub fn platform(&self) -> Option<String> {
        match (&self.os, &self.architecture) {
            (Some(os), Some(arch)) => Some(format!("{os}/{arch}")),
            _ => None,
        }
    }
}

/// Read [`ImageFacts`] out of an OCI image configuration.
///
/// The shape is the OCI image config: `architecture` and `os` at the top, and
/// `config.WorkingDir` / `config.ExposedPorts` nested inside. A runner reports
/// it verbatim, so this parses what Docker itself would have answered — which
/// is the point: the answer is the same, only the machine that knew it is
/// different.
///
/// Unreadable or absent fields come back as `None` rather than as defaults. A
/// missing `WORKDIR` and a `WORKDIR` of `/` are different instructions, and a
/// consumer that cannot tell them apart extracts from the wrong directory.
#[must_use]
pub fn image_facts_from_config(config: &serde_json::Value) -> ImageFacts {
    let nested = config.get("config");

    let working_dir = nested
        .and_then(|c| c.get("WorkingDir"))
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // `ExposedPorts` is an object whose keys are `"8080/tcp"`; the values are
    // empty objects and carry nothing.
    let mut exposed_ports: Vec<u16> = nested
        .and_then(|c| c.get("ExposedPorts"))
        .and_then(serde_json::Value::as_object)
        .map(|ports| {
            ports
                .keys()
                .filter_map(|spec| spec.split('/').next()?.parse::<u16>().ok())
                .collect()
        })
        .unwrap_or_default();
    exposed_ports.sort_unstable();
    exposed_ports.dedup();

    ImageFacts {
        working_dir,
        exposed_ports,
        architecture: config
            .get("architecture")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        os: config
            .get("os")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temps_build_protocol::{ArtifactRef, ENVELOPE_SCHEMA};

    fn envelope(digest: Option<&str>, artifacts: usize, elapsed_ms: i64) -> BuildResultEnvelope {
        let started = chrono::DateTime::UNIX_EPOCH;
        BuildResultEnvelope {
            schema: ENVELOPE_SCHEMA.to_string(),
            build_id: uuid::Uuid::nil(),
            digest: digest.map(str::to_string),
            platforms: vec!["linux/amd64".to_string()],
            config: None,
            size_bytes: Some(1234),
            artifacts: (0..artifacts)
                .map(|i| ArtifactRef {
                    path: format!("dist/file-{i}"),
                    media_type: "application/octet-stream".to_string(),
                    size_bytes: 1,
                    digest: "sha256:0".to_string(),
                })
                .collect(),
            scan: None,
            started_at: started,
            finished_at: started + chrono::TimeDelta::milliseconds(elapsed_ms),
        }
    }

    /// The ordinary case: a build that produced an image becomes the result
    /// the deployment path already knows how to handle, with nothing invented.
    #[test]
    fn an_envelope_with_an_image_narrows_to_a_build_result() {
        let result = narrow_to_image(envelope(Some("sha256:abc"), 0, 4_500), "app:tag")
            .expect("an envelope carrying an image narrows");

        assert_eq!(result.image_id, "sha256:abc", "the digest is the image id");
        assert_eq!(
            result.image_name, "app:tag",
            "the name comes from the request, not the runner: the runner does \
             not get to decide what the control plane calls its image"
        );
        assert_eq!(result.size_bytes, 1234);
        assert_eq!(
            result.build_duration_ms, 4_500,
            "duration is computed from the runner's own clock readings, since \
             the control plane never saw the build start"
        );
    }

    /// The reason this adapter exists. A recipe that produces a desktop binary
    /// reports artifacts and no digest, and the deployment path deploys images.
    /// Failing here — with the count of what it *did* produce — is the whole
    /// point of having made `digest` optional in the contract instead of
    /// pushing "maybe there is no image" into every consumer.
    #[test]
    fn an_envelope_without_an_image_is_refused_and_says_what_it_did_produce() {
        let err = narrow_to_image(envelope(None, 3, 1_000), "app:tag")
            .expect_err("a build with no image cannot become an image deployment");

        let message = err.to_string();
        assert!(
            message.contains("produced no image"),
            "the error must name the actual problem; got: {message}"
        );
        assert!(
            message.contains('3'),
            "the error must say what the build did produce, so the reader can \
             tell a broken build from a build of the wrong kind; got: {message}"
        );
        assert!(
            message.contains("artifact path"),
            "an error that only says no is a puzzle; this one points at where \
             such a build belongs. got: {message}"
        );
    }

    /// A runner that reports no size must not make the control plane guess a
    /// large one. Zero is the honest unknown here, and it is recorded rather
    /// than inferred, because only the machine holding the image can weigh it.
    #[test]
    fn a_missing_size_is_reported_as_zero_rather_than_estimated() {
        let mut e = envelope(Some("sha256:abc"), 0, 1);
        e.size_bytes = None;
        let result = narrow_to_image(e, "app:tag").expect("an envelope still narrows without size");
        assert_eq!(result.size_bytes, 0);
    }

    /// A clock that ran backwards between the two readings must not produce a
    /// duration that wraps into an enormous positive number.
    #[test]
    fn a_backwards_clock_does_not_produce_an_absurd_duration() {
        let result = narrow_to_image(envelope(Some("sha256:abc"), 0, -5_000), "app:tag")
            .expect("an envelope with odd timestamps still narrows");
        assert_eq!(
            result.build_duration_ms, 0,
            "a negative interval is clamped, not wrapped: an unsigned cast of \
             a negative millisecond count reads as roughly 584 million years"
        );
    }

    /// The default policy keeps every build exactly where it is. Introducing
    /// the router must not move anything on its own — the switch and the
    /// behaviour are separate changes so that rolling back one is not rolling
    /// back the other.
    #[test]
    fn the_default_policy_places_every_build_locally() {
        let request = BuildRequest {
            image_name: "app:tag".to_string(),
            context_path: PathBuf::from("/tmp"),
            dockerfile_path: None,
            build_args: Default::default(),
            build_args_buildkit: Default::default(),
            platform: None,
            log_path: PathBuf::from("/tmp/log"),
        };
        assert!(
            matches!(AlwaysLocal.placement_for(&request), BuildPlacement::Local),
            "the default policy must not move a build"
        );
    }
}

#[cfg(test)]
mod image_facts_tests {
    use super::*;

    /// The shape a runner reports, verbatim from the OCI image config. The
    /// answer must be the one the daemon would have given — only the machine
    /// that knew it is different.
    #[test]
    fn an_oci_config_yields_the_facts_the_pipeline_reads() {
        let config = serde_json::json!({
            "architecture": "amd64",
            "os": "linux",
            "config": {
                "WorkingDir": "/app",
                "ExposedPorts": { "8080/tcp": {}, "443/tcp": {} },
                "Env": ["PATH=/usr/bin"]
            }
        });

        let facts = image_facts_from_config(&config);
        assert_eq!(facts.working_dir.as_deref(), Some("/app"));
        assert_eq!(
            facts.exposed_ports,
            vec![443, 8080],
            "ports come back parsed and ordered, so a caller never depends on \
             the order a map happened to iterate in"
        );
        assert_eq!(facts.platform().as_deref(), Some("linux/amd64"));
    }

    /// A missing WORKDIR and a WORKDIR of `/` are different instructions. A
    /// consumer that cannot tell them apart extracts from the wrong directory
    /// and reports success.
    #[test]
    fn an_absent_workdir_is_none_and_not_a_default() {
        let config = serde_json::json!({ "config": { "ExposedPorts": {} } });
        assert_eq!(image_facts_from_config(&config).working_dir, None);

        let empty = serde_json::json!({ "config": { "WorkingDir": "" } });
        assert_eq!(
            image_facts_from_config(&empty).working_dir,
            None,
            "an empty string is Docker's way of saying unset, not a path"
        );

        let root = serde_json::json!({ "config": { "WorkingDir": "/" } });
        assert_eq!(
            image_facts_from_config(&root).working_dir.as_deref(),
            Some("/"),
            "a WORKDIR of / is a real instruction and must survive"
        );
    }

    /// A port spec this parser does not understand must be dropped, not
    /// guessed at. Publishing a port nobody asked for is worse than
    /// publishing none.
    #[test]
    fn unparseable_port_specs_are_dropped_rather_than_guessed() {
        let config = serde_json::json!({
            "config": { "ExposedPorts": { "8080/tcp": {}, "not-a-port/tcp": {}, "99999/tcp": {} } }
        });
        assert_eq!(
            image_facts_from_config(&config).exposed_ports,
            vec![8080],
            "99999 does not fit a port and neither does a word; both are \
             dropped rather than turned into something plausible"
        );
    }

    /// An envelope from a native build carries no image config at all. The
    /// facts must be empty rather than invented, so a caller that needs them
    /// discovers it has none.
    #[test]
    fn an_empty_config_yields_no_facts() {
        let facts = image_facts_from_config(&serde_json::json!({}));
        assert_eq!(facts, ImageFacts::default());
        assert_eq!(
            facts.platform(),
            None,
            "half a platform is not a platform: with no os and no architecture \
             there is nothing to format"
        );
    }
}

#[cfg(test)]
mod recall_tests {
    use super::*;

    struct NeverBuilds;

    #[async_trait]
    impl ImageBuilder for NeverBuilds {
        async fn build_image(&self, _: BuildRequest) -> Result<BuildResult, BuilderError> {
            unreachable!("these tests never build locally")
        }
        async fn build_image_with_callback(
            &self,
            _: BuildRequestWithCallback,
        ) -> Result<BuildResult, BuilderError> {
            unreachable!("these tests never build locally")
        }
        async fn import_image(&self, _: PathBuf, _: &str) -> Result<String, BuilderError> {
            unreachable!()
        }
        async fn save_image(&self, _: &str, _: &Path) -> Result<(), BuilderError> {
            unreachable!()
        }
        async fn extract_from_image(
            &self,
            _: &str,
            _: &str,
            _: &Path,
        ) -> Result<(), BuilderError> {
            unreachable!()
        }
        async fn list_images(&self) -> Result<Vec<String>, BuilderError> {
            unreachable!()
        }
        async fn remove_image(&self, _: &str) -> Result<(), BuilderError> {
            unreachable!()
        }
        /// Stands in for the daemon. Reaching here means the router asked a
        /// daemon about an image the daemon never saw, which is the bug this
        /// whole design exists to prevent.
        async fn inspect_image(&self, image_name: &str) -> Result<ImageInfo, BuilderError> {
            Err(BuilderError::BuildFailed(format!(
                "fell through to the daemon for {image_name}"
            )))
        }
        fn get_native_platform(&self) -> String {
            "linux/amd64".to_string()
        }
    }

    fn router() -> RoutedImageBuilder {
        RoutedImageBuilder::new(Arc::new(AlwaysLocal), Arc::new(NeverBuilds))
    }

    fn facts() -> ImageFacts {
        ImageFacts {
            working_dir: Some("/app".to_string()),
            exposed_ports: vec![8080],
            architecture: Some("amd64".to_string()),
            os: Some("linux".to_string()),
        }
    }

    /// The whole point: a consumer asks the same trait the same question and
    /// gets a true answer, without knowing the build ran elsewhere.
    #[tokio::test]
    async fn inspect_answers_from_the_hosted_report_and_never_touches_the_daemon() {
        let router = router();
        router.remember("app:sha", facts(), 9_000);

        let info = router
            .inspect_image("app:sha")
            .await
            .expect("a remembered image answers without a daemon");

        assert_eq!(info.working_dir.as_deref(), Some("/app"));
        assert_eq!(info.platform, "linux/amd64");
        assert_eq!(info.size_bytes, 9_000);
        assert_eq!(
            info.created, None,
            "no creation time was witnessed by anyone here; a value invented \
             would be indistinguishable from a real one"
        );
    }

    /// An image this router did not produce is the daemon's business. Falling
    /// through must stay the default, or a locally built image would be
    /// answered from an empty memory.
    #[tokio::test]
    async fn an_unknown_image_falls_through_to_the_daemon() {
        let err = router()
            .inspect_image("someone-elses:tag")
            .await
            .expect_err("the stand-in daemon always refuses");
        assert!(
            err.to_string().contains("fell through to the daemon"),
            "an image nobody remembers must be asked of the daemon; got: {err}"
        );
    }

    /// The memo is keyed by image name and bounded. Image names are minted per
    /// deployment, so an unbounded map here would leak on the busiest machine
    /// in the cluster.
    #[tokio::test]
    async fn the_memory_is_bounded_and_drops_the_oldest_first() {
        let router = router();
        for i in 0..(REMEMBERED_HOSTED_BUILDS + 10) {
            router.remember(&format!("app:{i}"), facts(), i as u64);
        }

        assert!(
            router.recall("app:0").is_none(),
            "the oldest entries must be gone, or this is a leak with extra steps"
        );
        let newest = format!("app:{}", REMEMBERED_HOSTED_BUILDS + 9);
        assert!(
            router.recall(&newest).is_some(),
            "the most recent build is the one a downstream job is about to ask about"
        );
        let held = router.hosted.lock().expect("uncontended in a test").len();
        assert_eq!(held, REMEMBERED_HOSTED_BUILDS, "the cap is a cap");
    }

    /// Rebuilding the same tag must replace what is remembered, not queue a
    /// second answer behind the first. A stale `WORKDIR` would make the next
    /// extraction read the previous build's directory.
    #[tokio::test]
    async fn rebuilding_a_tag_replaces_what_is_remembered() {
        let router = router();
        router.remember("app:latest", facts(), 1);

        let mut newer = facts();
        newer.working_dir = Some("/srv".to_string());
        router.remember("app:latest", newer, 2);

        let (recalled, size) = router.recall("app:latest").expect("still remembered");
        assert_eq!(recalled.working_dir.as_deref(), Some("/srv"));
        assert_eq!(size, 2);
        assert_eq!(
            router.hosted.lock().expect("uncontended").len(),
            1,
            "the same tag holds one memo, not a history"
        );
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    fn a_request() -> BuildRequest {
        BuildRequest {
            image_name: "app:sha".to_string(),
            context_path: PathBuf::from("/srv/build/app"),
            dockerfile_path: Some(PathBuf::from("backend/Dockerfile")),
            build_args: std::collections::HashMap::from([(
                "VERSION".to_string(),
                "1".to_string(),
            )]),
            build_args_buildkit: std::collections::HashMap::new(),
            platform: Some("linux/arm64".to_string()),
            log_path: PathBuf::from("/tmp/log"),
        }
    }

    fn policy(program: Option<&str>) -> ConfiguredBuildPolicy {
        ConfiguredBuildPolicy::new(
            program.map(PathBuf::from),
            42,
            Some(7),
            temps_build_protocol::Priority::Development,
            Duration::from_secs(600),
            "/usr/bin:/bin".to_string(),
            PathBuf::from("/home/build"),
        )
    }

    /// Every row that exists today has no build program, and every one of them
    /// must keep building exactly where it does now. A router that moved a
    /// build because someone deployed a new version would be a router nobody
    /// could roll back.
    #[test]
    fn no_configured_program_keeps_the_build_local() {
        assert!(
            matches!(policy(None).placement_for(&a_request()), BuildPlacement::Local),
            "an unconfigured project must not be moved by the mere presence of \
             this code"
        );
    }

    /// The plan carries what the host decided and nothing the request asked
    /// for: the program, the directory, the environment's identity, and an
    /// empty credential set.
    #[test]
    fn a_configured_program_produces_a_plan_the_request_could_not_have_chosen() {
        let BuildPlacement::Hosted(plan) = policy(Some("/usr/local/bin/temps-build"))
            .placement_for(&a_request())
        else {
            panic!("a configured program must place the build off the daemon")
        };

        assert_eq!(plan.program, PathBuf::from("/usr/local/bin/temps-build"));
        assert_eq!(
            plan.working_dir,
            PathBuf::from("/srv/build/app"),
            "the build runs in the context the pipeline prepared"
        );
        assert!(
            plan.credentials.is_empty(),
            "the build step gets no credential: it runs a dependency tree's \
             install scripts, and the token it never held is the one it cannot \
             leak"
        );
        assert_eq!(plan.request.requester.project_id, 42);
        assert_eq!(plan.request.requester.environment_id, Some(7));
        assert_eq!(
            plan.request.cache.project_id, 42,
            "cache is scoped to the project, which is what keeps one client's \
             layers out of another's build"
        );
    }

    /// The platform the deployment path asked for must survive into the
    /// target, or a build lands on the wrong architecture and fails late.
    #[test]
    fn the_requested_platform_reaches_the_target() {
        let BuildPlacement::Hosted(plan) =
            policy(Some("/bin/true")).placement_for(&a_request())
        else {
            panic!("configured")
        };
        assert_eq!(plan.request.target.os, temps_build_protocol::Os::Linux);
        assert_eq!(plan.request.target.arch, temps_build_protocol::Arch::Arm64);
    }

    /// A platform this protocol cannot name must fall back to this host's own
    /// rather than be approximated. Sending a build to a plausible-looking
    /// wrong architecture fails confusingly and late.
    #[test]
    fn an_unnameable_platform_falls_back_instead_of_being_guessed() {
        assert_eq!(parse_platform("linux/riscv64"), None);
        assert_eq!(parse_platform("plan9/amd64"), None);
        assert_eq!(parse_platform("garbage"), None);
        assert_eq!(
            parse_platform("darwin/arm64"),
            Some((temps_build_protocol::Os::MacOs, temps_build_protocol::Arch::Arm64)),
            "darwin and macos are the same platform under two names, and a \
             desktop build will arrive spelled either way"
        );

        let mut request = a_request();
        request.platform = Some("linux/riscv64".to_string());
        let BuildPlacement::Hosted(plan) = policy(Some("/bin/true")).placement_for(&request) else {
            panic!("configured")
        };
        assert_eq!(
            plan.request.target.arch,
            temps_build_protocol::Arch::Amd64,
            "an unnameable platform lands on this host's own, which is what a \
             local build would have produced anyway"
        );
    }

    /// The Dockerfile the pipeline resolved must reach the recipe. Defaulting
    /// silently would build the wrong image for any project whose Dockerfile
    /// is not at the root — which is both of the CRM's.
    #[test]
    fn the_resolved_dockerfile_reaches_the_recipe() {
        let BuildPlacement::Hosted(plan) = policy(Some("/bin/true")).placement_for(&a_request()) else {
            panic!("configured")
        };
        match plan.request.recipe {
            temps_build_protocol::BuildRecipe::Dockerfile { path, build_args, .. } => {
                assert_eq!(path, "backend/Dockerfile");
                assert_eq!(build_args.get("VERSION").map(String::as_str), Some("1"));
            }
            other => panic!("a Dockerfile request must stay a Dockerfile recipe, got: {other:?}"),
        }
    }
}
