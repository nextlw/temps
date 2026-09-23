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

pub struct RoutedImageBuilder {
    policy: Arc<dyn BuildPolicy>,
    local: Arc<dyn ImageBuilder>,
}

impl RoutedImageBuilder {
    pub fn new(policy: Arc<dyn BuildPolicy>, local: Arc<dyn ImageBuilder>) -> Self {
        Self { policy, local }
    }

    async fn build_hosted(
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
                Self::build_hosted(*plan, &image_name).await
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
                Self::build_hosted(*plan, &image_name).await
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

    async fn inspect_image(&self, image_name: &str) -> Result<ImageInfo, BuilderError> {
        self.local.inspect_image(image_name).await
    }

    fn get_native_platform(&self) -> String {
        self.local.get_native_platform()
    }

    fn discovered_platform(&self) -> Option<String> {
        self.local.discovered_platform()
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
