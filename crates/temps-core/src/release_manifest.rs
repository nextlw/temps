// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use serde::Deserialize;
use thiserror::Error;

#[derive(Clone, Copy)]
pub enum ImageKind {
    DaemonNodejs,
    DaemonPython,
    DaemonAll,
    SandboxNode,
    SandboxBun,
    SandboxPython,
    SandboxRust,
    SandboxGo,
    SandboxFull,
    PreviewGateway,
}

impl ImageKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DaemonNodejs => "daemon_nodejs",
            Self::DaemonPython => "daemon_python",
            Self::DaemonAll => "daemon_all",
            Self::SandboxNode => "sandbox_node",
            Self::SandboxBun => "sandbox_bun",
            Self::SandboxPython => "sandbox_python",
            Self::SandboxRust => "sandbox_rust",
            Self::SandboxGo => "sandbox_go",
            Self::SandboxFull => "sandbox_full",
            Self::PreviewGateway => "preview_gateway",
        }
    }
}

/// Registry owner the runtime images are bound to when the build does not name
/// one. It stays the upstream owner so a plain local build (or one cut from
/// gotempsh/temps) keeps accepting the manifests it always accepted; releases
/// from the fork resolve to their own owner through the variables read by
/// [`image_namespace`].
pub const DEFAULT_IMAGE_NAMESPACE: &str = "gotempsh";

/// Repository name of each runtime image inside `ghcr.io/<namespace>/`. The
/// owner is not part of it because a fork publishes the same images under its
/// own namespace (see `.github/scripts/release_image_manifest.py`).
pub const IMAGES: [(ImageKind, &str); 10] = [
    (ImageKind::DaemonNodejs, "temps-sandbox-nodejs"),
    (ImageKind::DaemonPython, "temps-sandbox-python"),
    (ImageKind::DaemonAll, "temps-sandbox-all"),
    (ImageKind::SandboxNode, "temps-sandbox-node"),
    (ImageKind::SandboxBun, "temps-sandbox-bun"),
    (ImageKind::SandboxPython, "temps-sandbox-python"),
    (ImageKind::SandboxRust, "temps-sandbox-rust"),
    (ImageKind::SandboxGo, "temps-sandbox-go"),
    (ImageKind::SandboxFull, "temps-sandbox-full"),
    (ImageKind::PreviewGateway, "temps-preview-gateway"),
];

/// Resolves the registry owner with the same precedence as the Python script
/// that writes the manifest: `TEMPS_IMAGE_NAMESPACE`, then
/// `GITHUB_REPOSITORY_OWNER` (always set on GitHub Actions), then
/// [`DEFAULT_IMAGE_NAMESPACE`]. Both sides must agree or the build rejects the
/// manifest the release pipeline just produced.
pub fn image_namespace(explicit: Option<&str>, repository_owner: Option<&str>) -> String {
    explicit
        .filter(|value| !value.is_empty())
        .or(repository_owner.filter(|value| !value.is_empty()))
        .unwrap_or(DEFAULT_IMAGE_NAMESPACE)
        .to_ascii_lowercase()
}

/// Full GHCR repository of a runtime image (`name` from [`IMAGES`]) under the
/// resolved namespace; the manifest must reference exactly this repository.
pub fn image_repository(namespace: &str, name: &str) -> String {
    format!("ghcr.io/{namespace}/{name}")
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("manifest JSON is invalid: {source}")]
    Json {
        #[source]
        source: serde_json::Error,
    },
    #[error("revision '{revision}' must be 40 lowercase hexadecimal characters")]
    Revision { revision: String },
    #[error("image '{key}' must use {repo}@sha256:, got '{reference}'")]
    ImageReference {
        key: &'static str,
        repo: String,
        reference: String,
    },
    #[error("image '{key}' must have a 64-character lowercase sha256 digest, got '{digest}'")]
    Digest { key: &'static str, digest: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub revision: String,
    pub images: Images,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Images {
    daemon_nodejs: String,
    daemon_python: String,
    daemon_all: String,
    sandbox_node: String,
    sandbox_bun: String,
    sandbox_python: String,
    sandbox_rust: String,
    sandbox_go: String,
    sandbox_full: String,
    preview_gateway: String,
}

impl Images {
    pub fn get(&self, key: ImageKind) -> &str {
        match key {
            ImageKind::DaemonNodejs => &self.daemon_nodejs,
            ImageKind::DaemonPython => &self.daemon_python,
            ImageKind::DaemonAll => &self.daemon_all,
            ImageKind::SandboxNode => &self.sandbox_node,
            ImageKind::SandboxBun => &self.sandbox_bun,
            ImageKind::SandboxPython => &self.sandbox_python,
            ImageKind::SandboxRust => &self.sandbox_rust,
            ImageKind::SandboxGo => &self.sandbox_go,
            ImageKind::SandboxFull => &self.sandbox_full,
            ImageKind::PreviewGateway => &self.preview_gateway,
        }
    }
}

pub fn parse_manifest(contents: &str, namespace: &str) -> Result<Manifest, ManifestError> {
    let manifest: Manifest =
        serde_json::from_str(contents).map_err(|source| ManifestError::Json { source })?;
    if manifest.revision.len() != 40 || !is_lower_hex(&manifest.revision) {
        return Err(ManifestError::Revision {
            revision: manifest.revision,
        });
    }
    for (key, name) in IMAGES {
        let repo = image_repository(namespace, name);
        let reference = manifest.images.get(key);
        let digest = reference
            .strip_prefix(repo.as_str())
            .and_then(|rest| rest.strip_prefix("@sha256:"))
            .ok_or_else(|| ManifestError::ImageReference {
                key: key.as_str(),
                repo,
                reference: reference.to_string(),
            })?;
        if digest.len() != 64 || !is_lower_hex(digest) {
            return Err(ManifestError::Digest {
                key: key.as_str(),
                digest: digest.to_string(),
            });
        }
    }
    Ok(manifest)
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_for(namespace: &str) -> serde_json::Value {
        let images: serde_json::Map<String, serde_json::Value> = IMAGES
            .iter()
            .map(|(key, name)| {
                (
                    key.as_str().into(),
                    format!(
                        "{}@sha256:{}",
                        image_repository(namespace, name),
                        "a".repeat(64)
                    )
                    .into(),
                )
            })
            .collect();
        serde_json::json!({"revision": "b".repeat(40), "images": images})
    }

    fn valid() -> serde_json::Value {
        valid_for(DEFAULT_IMAGE_NAMESPACE)
    }

    fn parse(value: &serde_json::Value) -> Result<Manifest, ManifestError> {
        parse_manifest(&value.to_string(), DEFAULT_IMAGE_NAMESPACE)
    }

    #[test]
    fn accepts_complete_manifest() {
        let manifest = parse(&valid()).expect("valid manifest");
        assert_eq!(
            manifest.images.get(ImageKind::DaemonNodejs),
            format!(
                "ghcr.io/gotempsh/temps-sandbox-nodejs@sha256:{}",
                "a".repeat(64)
            )
        );
    }

    #[test]
    fn accepts_fork_namespace_and_rejects_it_under_another_namespace() {
        // Regression: v0.1.0-nextlw.3 failed in build.rs because the fork's
        // manifest pointed at ghcr.io/nextlw/* while the repositories were
        // fixed to ghcr.io/gotempsh/*.
        let fork = valid_for("nextlw").to_string();
        let manifest = parse_manifest(&fork, "nextlw").expect("fork manifest");
        assert_eq!(
            manifest.images.get(ImageKind::PreviewGateway),
            format!(
                "ghcr.io/nextlw/temps-preview-gateway@sha256:{}",
                "a".repeat(64)
            )
        );
        assert!(matches!(
            parse_manifest(&fork, DEFAULT_IMAGE_NAMESPACE),
            Err(ManifestError::ImageReference {
                key: "daemon_nodejs",
                ..
            })
        ));
    }

    #[test]
    fn namespace_follows_the_manifest_script_precedence() {
        assert_eq!(image_namespace(None, None), DEFAULT_IMAGE_NAMESPACE);
        assert_eq!(image_namespace(Some(""), Some("")), DEFAULT_IMAGE_NAMESPACE);
        assert_eq!(image_namespace(None, Some("NextLW")), "nextlw");
        assert_eq!(image_namespace(Some(""), Some("nextlw")), "nextlw");
        assert_eq!(image_namespace(Some("mirror"), Some("nextlw")), "mirror");
    }

    #[test]
    fn rejects_missing_extra_wrong_repo_and_bad_digest() {
        let mut missing = valid();
        missing["images"]
            .as_object_mut()
            .expect("images object")
            .remove("daemon_all");
        assert!(parse(&missing).is_err());
        let mut extra = valid();
        extra["images"]["unexpected"] = "value".into();
        assert!(parse(&extra).is_err());
        let mut wrong = valid();
        wrong["images"]["daemon_nodejs"] =
            format!("ghcr.io/other/repo@sha256:{}", "a".repeat(64)).into();
        assert!(matches!(
            parse(&wrong),
            Err(ManifestError::ImageReference {
                key: "daemon_nodejs",
                ..
            })
        ));
        let mut digest = valid();
        digest["images"]["sandbox_go"] = "ghcr.io/gotempsh/temps-sandbox-go@sha256:invalid".into();
        assert!(matches!(
            parse(&digest),
            Err(ManifestError::Digest {
                key: "sandbox_go",
                ..
            })
        ));
        let mut uppercase = valid();
        uppercase["revision"] = "A".repeat(40).into();
        assert!(matches!(
            parse(&uppercase),
            Err(ManifestError::Revision { .. })
        ));
    }

    #[test]
    fn compiled_constants_match_the_manifest_or_local_fallback() {
        let compiled_path = option_env!("TEMPS_RELEASE_IMAGE_MANIFEST");
        match compiled_path {
            Some(path) => {
                let contents =
                    std::fs::read_to_string(path).expect("compiled manifest still exists");
                // Same resolution build.rs ran with, so the constants are
                // checked against the namespace they were validated for.
                let namespace = image_namespace(
                    option_env!("TEMPS_IMAGE_NAMESPACE"),
                    option_env!("GITHUB_REPOSITORY_OWNER"),
                );
                let manifest =
                    parse_manifest(&contents, &namespace).expect("compiled manifest is valid");
                assert_eq!(
                    crate::release_images::REVISION,
                    Some(manifest.revision.as_str())
                );
                assert_eq!(
                    crate::release_images::DAEMON_NODEJS,
                    Some(manifest.images.get(ImageKind::DaemonNodejs))
                );
                assert_eq!(
                    crate::release_images::DAEMON_PYTHON,
                    Some(manifest.images.get(ImageKind::DaemonPython))
                );
                assert_eq!(
                    crate::release_images::DAEMON_ALL,
                    Some(manifest.images.get(ImageKind::DaemonAll))
                );
                assert_eq!(
                    crate::release_images::SANDBOX_NODE,
                    Some(manifest.images.get(ImageKind::SandboxNode))
                );
                assert_eq!(
                    crate::release_images::SANDBOX_BUN,
                    Some(manifest.images.get(ImageKind::SandboxBun))
                );
                assert_eq!(
                    crate::release_images::SANDBOX_PYTHON,
                    Some(manifest.images.get(ImageKind::SandboxPython))
                );
                assert_eq!(
                    crate::release_images::SANDBOX_RUST,
                    Some(manifest.images.get(ImageKind::SandboxRust))
                );
                assert_eq!(
                    crate::release_images::SANDBOX_GO,
                    Some(manifest.images.get(ImageKind::SandboxGo))
                );
                assert_eq!(
                    crate::release_images::SANDBOX_FULL,
                    Some(manifest.images.get(ImageKind::SandboxFull))
                );
                assert_eq!(
                    crate::release_images::PREVIEW_GATEWAY,
                    Some(manifest.images.get(ImageKind::PreviewGateway))
                );
            }
            None => {
                assert_eq!(crate::release_images::REVISION, None);
                assert_eq!(crate::release_images::DAEMON_NODEJS, None);
                assert_eq!(crate::release_images::PREVIEW_GATEWAY, None);
            }
        }
    }
}
