// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One-off data-transfer containers shared by every feature that copies a
//! database from somewhere else into a temps-managed service.
//!
//! The platform importer (`temps-import`) and the "populate a database of an
//! existing service" operation ([`crate::service_populate`]) both copy data
//! the same way: an official database client image runs a short shell
//! pipeline (`pg_dump … | psql …`) that reads the source URL from `$SRC` and
//! the destination URL from `$DST`, bounded by a hard timeout after which the
//! container is force-removed. Keeping that runner here means both callers
//! share one implementation of the timeout, the cleanup and the log capture.

use bollard::Docker;
use futures::StreamExt;
use std::time::Duration;
use thiserror::Error;

/// Hard cap on how long a data-transfer container may run in production.
///
/// A dump/restore against an unreachable or very slow source database must
/// not hang a Tokio worker forever — a few of these on a small (cpx22-class)
/// box would starve everything else. Real transfers on a reachable database
/// finish in seconds to minutes.
pub const DATA_TRANSFER_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How many characters of a container's log tail an error carries.
const LOG_TAIL_CHARS: usize = 500;

/// Why a transfer container did not finish successfully.
#[derive(Debug, Error)]
pub enum DataTransferError {
    #[error("failed to pull transfer image '{image}': {reason}")]
    ImagePull { image: String, reason: String },

    #[error("failed to create transfer container '{name}': {reason}")]
    ContainerCreate { name: String, reason: String },

    #[error("failed to start transfer container '{name}': {reason}")]
    ContainerStart { name: String, reason: String },

    #[error("transfer container failed: {reason} — {log_tail}")]
    Wait { reason: String, log_tail: String },

    #[error(
        "transfer timed out after {timeout_secs}s — the source database may be \
         unreachable or too slow to respond; check network connectivity \
         and consider running the dump/restore manually. Log tail: {log_tail}"
    )]
    TimedOut { timeout_secs: u64, log_tail: String },

    #[error("transfer exited with status {status}: {log_tail}")]
    Exit { status: i64, log_tail: String },
}

/// Everything needed to start one transfer container.
pub struct TransferContainerSpec<'a> {
    /// Official client image (e.g. `postgres:18-alpine`).
    pub image: &'a str,
    /// Shell pipeline run with `sh -c`; reads `$SRC` and `$DST`.
    pub command: &'a str,
    /// Source connection URL, exposed to the command as `$SRC`.
    pub source_url: &'a str,
    /// Destination connection URL, exposed to the command as `$DST`.
    pub destination_url: &'a str,
    /// Docker network mode. Production callers use `host` so the container
    /// reaches both the external source and the locally published port of
    /// the managed service.
    pub network_mode: &'a str,
    /// Prefix of the container name; a random suffix is appended.
    pub name_prefix: &'a str,
    /// Bound after which the container is force-removed.
    pub timeout: Duration,
}

/// Run a one-off transfer container to completion.
///
/// The container is always removed: after a clean exit, after a failed exit,
/// and on timeout (force-removal is what cancels a hung transfer).
pub async fn run_transfer_container(
    docker: &Docker,
    spec: &TransferContainerSpec<'_>,
) -> Result<(), DataTransferError> {
    use bollard::models::{ContainerCreateBody, HostConfig};
    use bollard::query_parameters::{
        CreateContainerOptionsBuilder, CreateImageOptions, StartContainerOptions,
    };

    // Ensure the image exists (no-op when already pulled)
    let mut pull = docker.create_image(
        Some(CreateImageOptions {
            from_image: Some(spec.image.to_string()),
            ..Default::default()
        }),
        None,
        None,
    );
    while let Some(item) = pull.next().await {
        if let Err(e) = item {
            return Err(DataTransferError::ImagePull {
                image: spec.image.to_string(),
                reason: e.to_string(),
            });
        }
    }

    let name = format!(
        "{}-{}",
        spec.name_prefix,
        &uuid::Uuid::new_v4().to_string()[..8]
    );
    let container = docker
        .create_container(
            Some(CreateContainerOptionsBuilder::new().name(&name).build()),
            ContainerCreateBody {
                image: Some(spec.image.to_string()),
                cmd: Some(vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    spec.command.to_string(),
                ]),
                env: Some(vec![
                    format!("SRC={}", spec.source_url),
                    format!("DST={}", spec.destination_url),
                ]),
                host_config: Some(HostConfig {
                    network_mode: Some(spec.network_mode.to_string()),
                    auto_remove: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| DataTransferError::ContainerCreate {
            name: name.clone(),
            reason: e.to_string(),
        })?;

    if let Err(e) = docker
        .start_container(&container.id, None::<StartContainerOptions>)
        .await
    {
        remove_container_forcefully(docker, &container.id).await;
        return Err(DataTransferError::ContainerStart {
            name,
            reason: e.to_string(),
        });
    }

    // Bounded wait — an unreachable or hung source database must not tie up
    // a Tokio worker forever. On timeout the container is already removed.
    let status = wait_for_container(docker, &container.id, spec.timeout).await?;

    // Capture the tail of the logs for the error message before removal
    let logs = container_log_tail(docker, &container.id).await;
    remove_container_forcefully(docker, &container.id).await;

    if status == 0 {
        Ok(())
    } else {
        Err(DataTransferError::Exit {
            status,
            log_tail: logs.chars().take(LOG_TAIL_CHARS).collect(),
        })
    }
}

/// Wait for `container_id` to finish, bounded by `timeout`. On success
/// returns the exit status code. On timeout or a wait-stream error, force-
/// removes the container and returns a descriptive error including the log
/// tail — callers must not leak a hung container back to Docker.
pub async fn wait_for_container(
    docker: &Docker,
    container_id: &str,
    timeout: Duration,
) -> Result<i64, DataTransferError> {
    use bollard::query_parameters::WaitContainerOptions;

    let mut wait = docker.wait_container(container_id, None::<WaitContainerOptions>);
    // bollard's wait errors on nonzero exit codes with an often-empty
    // message — treat it as a failed status and let the log tail explain.
    match tokio::time::timeout(timeout, wait.next()).await {
        Ok(Some(Ok(result))) => Ok(result.status_code),
        Ok(Some(Err(bollard::errors::Error::DockerContainerWaitError { code, .. }))) => Ok(code),
        Ok(Some(Err(e))) => {
            let logs = container_log_tail(docker, container_id).await;
            remove_container_forcefully(docker, container_id).await;
            Err(DataTransferError::Wait {
                reason: e.to_string(),
                log_tail: logs.chars().take(400).collect(),
            })
        }
        Ok(None) => Ok(-1),
        Err(_) => {
            let logs = container_log_tail(docker, container_id).await;
            remove_container_forcefully(docker, container_id).await;
            Err(DataTransferError::TimedOut {
                timeout_secs: timeout.as_secs(),
                log_tail: logs.chars().take(400).collect(),
            })
        }
    }
}

async fn remove_container_forcefully(docker: &Docker, container_id: &str) {
    use bollard::query_parameters::RemoveContainerOptions;
    let _ = docker
        .remove_container(
            container_id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;
}

/// Last 20 lines of a container's stdout + stderr.
pub async fn container_log_tail(docker: &Docker, container_id: &str) -> String {
    use bollard::query_parameters::LogsOptionsBuilder;

    let mut stream = docker.logs(
        container_id,
        Some(
            LogsOptionsBuilder::new()
                .stdout(true)
                .stderr(true)
                .tail("20")
                .build(),
        ),
    );
    let mut output = String::new();
    while let Some(Ok(chunk)) = stream.next().await {
        output.push_str(&chunk.to_string());
    }
    output
}

/// Percent-encode a DSN userinfo component. Everything outside the RFC 3986
/// unreserved set is encoded — including '%' itself, which the url crate's
/// userinfo setters pass through unchanged.
pub fn percent_encode_userinfo(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

/// Image + shell command per service type for the importer's one-off data
/// transfer into a freshly created service. The command reads `$SRC` (source
/// platform URL) and `$DST` (new local URL).
pub fn dump_restore_command(plan_type: &str) -> Option<(String, String)> {
    // Each command first waits for the freshly created managed service to
    // accept connections — the database container may still be initializing
    // when the transfer starts — then runs the dump/restore.
    match plan_type {
        "postgres" | "postgresql" => Some((
            "postgres:16-alpine".to_string(),
            "for i in $(seq 1 45); do pg_isready -d \"$DST\" >/dev/null 2>&1 && break; sleep 2; done; pg_dump --no-owner --no-privileges \"$SRC\" | psql \"$DST\"".to_string(),
        )),
        "mysql" | "mariadb" => Some((
            "mariadb:11".to_string(),
            "for i in $(seq 1 45); do mariadb --skip-ssl \"--uri=$DST\" -e 'SELECT 1' >/dev/null 2>&1 && break; sleep 2; done; mariadb-dump --skip-ssl \"--uri=$SRC\" | mariadb \"--uri=$DST\"".to_string(),
        )),
        "mongodb" | "mongo" => Some((
            "mongo:7".to_string(),
            "for i in $(seq 1 45); do mongosh \"$DST\" --quiet --eval 'db.runCommand({ping:1})' >/dev/null 2>&1 && break; sleep 2; done; mongodump --uri=\"$SRC\" --archive | mongorestore --uri=\"$DST\" --archive"
                .to_string(),
        )),
        _ => None,
    }
}

/// Shell pipeline that copies a PostgreSQL database into an existing one.
///
/// Same mechanism as the importer's command (`pg_dump --no-owner
/// --no-privileges "$SRC" | psql "$DST"`), made strict because the result is
/// reported as a success or a failure to an operator:
///
/// - `set -o pipefail` — without it the pipeline's status is `psql`'s alone,
///   so a `pg_dump` that cannot even authenticate feeds `psql` an empty
///   script and the copy "succeeds" into an empty database;
/// - `ON_ERROR_STOP=1` — `psql` otherwise exits 0 after SQL errors;
/// - `--single-transaction` — a failed copy rolls back completely, leaving
///   the destination as empty as it was before, so a retry needs no cleanup;
/// - `-X` — ignore any `psqlrc` in the image.
pub fn postgres_populate_command() -> &'static str {
    "set -o pipefail; pg_dump --no-owner --no-privileges \"$SRC\" | psql -X -q -v ON_ERROR_STOP=1 --single-transaction \"$DST\" >/dev/null"
}

/// Official client image for a destination server of the given major version.
///
/// `pg_dump` refuses to dump from a server newer than itself, but dumps from
/// every older server it supports (back to 9.2); `psql` then replays the dump
/// into the destination in the destination's own dialect. Taking the client
/// version from the destination therefore covers every supported direction
/// (same version, or an older source into a newer destination — the case of
/// a PG16 source and a PG18 managed service) and fails with pg_dump's
/// explicit "server version mismatch" message on a downgrade, which a dump
/// cannot do faithfully anyway.
pub fn postgres_client_image(destination_major: u32) -> String {
    format!("postgres:{}-alpine", destination_major)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn userinfo_encoding_covers_url_hostile_password_chars() {
        // '%' is the case url::Url::set_password gets wrong — it must be
        // encoded so libpq does not see an invalid percent-escape.
        assert_eq!(
            percent_encode_userinfo("=&v1d%ghuyL@i0^S3"),
            "%3D%26v1d%25ghuyL%40i0%5ES3"
        );
        assert_eq!(percent_encode_userinfo("plain-User_1.~"), "plain-User_1.~");
    }

    #[test]
    fn dump_commands_exist_for_data_bearing_types() {
        assert!(dump_restore_command("postgres").is_some());
        assert!(dump_restore_command("mariadb").is_some());
        assert!(dump_restore_command("mongodb").is_some());
        assert!(dump_restore_command("redis").is_none());
    }

    #[test]
    fn importer_postgres_command_is_unchanged() {
        let (image, command) = dump_restore_command("postgres").expect("postgres command");
        assert_eq!(image, "postgres:16-alpine");
        assert!(command.ends_with("pg_dump --no-owner --no-privileges \"$SRC\" | psql \"$DST\""));
    }

    #[test]
    fn populate_command_fails_on_any_error_and_is_atomic() {
        let command = postgres_populate_command();
        assert!(command.starts_with("set -o pipefail;"));
        assert!(command.contains("pg_dump --no-owner --no-privileges \"$SRC\""));
        assert!(command.contains("-v ON_ERROR_STOP=1"));
        assert!(command.contains("--single-transaction"));
        assert!(command.contains("\"$DST\""));
        // Credentials only ever reach the container through the environment.
        assert!(!command.contains("postgres://"));
    }

    #[test]
    fn client_image_follows_the_destination_major() {
        assert_eq!(postgres_client_image(18), "postgres:18-alpine");
        assert_eq!(postgres_client_image(16), "postgres:16-alpine");
    }

    /// Starts a container that sleeps far longer than the bound and confirms
    /// `wait_for_container` returns a timeout error promptly and
    /// force-removes the container instead of leaking it. Skips gracefully if
    /// Docker isn't available, per this repo's Docker-test convention.
    #[tokio::test]
    async fn wait_for_container_times_out_on_a_hung_container() {
        use bollard::models::ContainerCreateBody;
        use bollard::query_parameters::{
            CreateContainerOptionsBuilder, CreateImageOptions, InspectContainerOptions,
            StartContainerOptions,
        };

        let docker = match Docker::connect_with_local_defaults() {
            Ok(d) => d,
            Err(e) => {
                println!("Docker not available, skipping: {}", e);
                return;
            }
        };
        if docker.ping().await.is_err() {
            println!("Docker not available, skipping");
            return;
        }

        // CI runners start clean: pull the fixture image explicitly.
        let mut pull = docker.create_image(
            Some(CreateImageOptions {
                from_image: Some("busybox:latest".to_string()),
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(item) = pull.next().await {
            item.expect("pull busybox:latest fixture image");
        }

        let name = format!(
            "temps-transfer-test-hang-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let container = docker
            .create_container(
                Some(CreateContainerOptionsBuilder::new().name(&name).build()),
                ContainerCreateBody {
                    image: Some("busybox:latest".to_string()),
                    cmd: Some(vec![
                        "sh".to_string(),
                        "-c".to_string(),
                        "sleep 300".to_string(),
                    ]),
                    ..Default::default()
                },
            )
            .await
            .expect("create hung-container fixture");
        docker
            .start_container(&container.id, None::<StartContainerOptions>)
            .await
            .expect("start hung-container fixture");

        let started = std::time::Instant::now();
        let result = wait_for_container(&docker, &container.id, Duration::from_secs(2)).await;
        let elapsed = started.elapsed();

        let message = result
            .expect_err("a sleeping container must time out, not exit cleanly")
            .to_string();
        assert!(
            message.contains("timed out after 2s"),
            "error should name the bound that fired: {}",
            message
        );
        assert!(
            elapsed < Duration::from_secs(60),
            "must return promptly on timeout, not wait for the 300s sleep: took {:?}",
            elapsed
        );

        let inspect = docker
            .inspect_container(&container.id, None::<InspectContainerOptions>)
            .await;
        assert!(
            inspect.is_err(),
            "timed-out container should have been force-removed, but it still exists"
        );

        remove_container_forcefully(&docker, &container.id).await;
    }
}
