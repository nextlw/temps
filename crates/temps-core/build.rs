// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[path = "src/release_manifest.rs"]
mod release_manifest;

use release_manifest::{image_namespace, parse_manifest, ManifestError, IMAGES};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error)]
enum BuildError {
    #[error("failed to read release image manifest at {path}: {source}")]
    ReadManifest {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid release image manifest at {path}: {source}")]
    InvalidManifest {
        path: PathBuf,
        #[source]
        source: ManifestError,
    },
    #[error("image namespace {namespace:?} must match ^[a-z0-9-]+$")]
    InvalidNamespace { namespace: String },
    #[error("Cargo did not provide OUT_DIR for release image generation: {source}")]
    OutputDirectory {
        #[source]
        source: env::VarError,
    },
    #[error("failed to write generated release image constants at {path}: {source}")]
    WriteGenerated {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn main() -> Result<(), BuildError> {
    println!("cargo:rerun-if-env-changed=TEMPS_RELEASE_IMAGE_MANIFEST");
    println!("cargo:rerun-if-env-changed=TEMPS_IMAGE_NAMESPACE");
    println!("cargo:rerun-if-env-changed=GITHUB_REPOSITORY_OWNER");
    println!("cargo:rerun-if-changed=src/release_manifest.rs");
    // The manifest is written by .github/scripts/release_image_manifest.py
    // under ghcr.io/<owner>/, so it is validated against the same owner the
    // script resolved instead of a fixed upstream namespace.
    let namespace = image_namespace(
        env::var("TEMPS_IMAGE_NAMESPACE").ok().as_deref(),
        env::var("GITHUB_REPOSITORY_OWNER").ok().as_deref(),
    );
    // Fail here rather than at the first workspace INSERT: the database only
    // accepts owners shaped like a GHCR namespace.
    let namespace_is_valid = !namespace.is_empty()
        && namespace
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'));
    if !namespace_is_valid {
        return Err(BuildError::InvalidNamespace { namespace });
    }
    let manifest = env::var_os("TEMPS_RELEASE_IMAGE_MANIFEST")
        .map(|path| -> Result<_, BuildError> {
            println!("cargo:rerun-if-changed={}", Path::new(&path).display());
            let contents =
                fs::read_to_string(&path).map_err(|source| BuildError::ReadManifest {
                    path: PathBuf::from(&path),
                    source,
                })?;
            parse_manifest(&contents, &namespace).map_err(|source| BuildError::InvalidManifest {
                path: PathBuf::from(&path),
                source,
            })
        })
        .transpose()?;
    let mut generated = String::from("// SPDX-FileCopyrightText: 2024-2026 Temps Contributors\n// SPDX-License-Identifier: MIT OR Apache-2.0\n\n");
    let revision = manifest.as_ref().map(|value| value.revision.as_str());
    generated.push_str(&format!(
        "pub const REVISION: Option<&str> = {revision:?};\n"
    ));
    // Exposed so runtime checks accept managed images under the owner this
    // build embedded, not only under upstream's.
    generated.push_str("/// GHCR owner the embedded runtime images belong to.\n");
    generated.push_str(&format!(
        "pub const IMAGE_NAMESPACE: &str = {namespace:?};\n"
    ));
    for (key, _) in IMAGES {
        let value = manifest.as_ref().map(|value| value.images.get(key));
        generated.push_str(&format!(
            "pub const {}: Option<&str> = {value:?};\n",
            key.as_str().to_ascii_uppercase()
        ));
    }
    let out_dir = env::var("OUT_DIR").map_err(|source| BuildError::OutputDirectory { source })?;
    let output_path = Path::new(&out_dir).join("release_images.rs");
    fs::write(&output_path, generated).map_err(|source| BuildError::WriteGenerated {
        path: output_path,
        source,
    })?;
    Ok(())
}
