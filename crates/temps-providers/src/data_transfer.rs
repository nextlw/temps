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

/// Hard cap on pulling the client image. A registry that accepts the
/// connection and then stalls would otherwise hold the transfer before the
/// transfer timeout even starts counting.
pub const IMAGE_PULL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How many characters of a container's log tail an error carries.
const LOG_TAIL_CHARS: usize = 500;

/// Where the libpq password file lands inside a transfer container.
const PGPASS_DIR: &str = "/tmp";
const PGPASS_FILE: &str = "temps-transfer.pgpass";

/// Why a transfer container did not finish successfully.
#[derive(Debug, Error)]
pub enum DataTransferError {
    #[error("failed to pull transfer image '{image}': {reason}")]
    ImagePull { image: String, reason: String },

    #[error("failed to create transfer container '{name}': {reason}")]
    ContainerCreate { name: String, reason: String },

    #[error("failed to hand credentials to transfer container '{name}': {reason}")]
    Credentials { name: String, reason: String },

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

/// How the database credentials reach the transfer container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferCredentials {
    /// The URLs go into `$SRC`/`$DST` as given, password included. Only for
    /// clients without a password file (`mariadb`, `mongodump`).
    InUrl,
    /// PostgreSQL: `$SRC`/`$DST` carry no password; the passwords go into a
    /// libpq password file (mode 0600) uploaded between create and start and
    /// named by `PGPASSFILE`. Nothing secret appears in the container's
    /// `Config.Env`, its command or the host's process list.
    PgPassFile,
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
    /// How the passwords in the two URLs reach the container.
    pub credentials: TransferCredentials,
    /// Docker network mode. Production callers use `host` so the container
    /// reaches both the external source and the locally published port of
    /// the managed service.
    pub network_mode: &'a str,
    /// Prefix of the container name; a random suffix is appended.
    pub name_prefix: &'a str,
    /// Labels on the container, so a later process can find (and remove)
    /// transfers an earlier process left behind.
    pub labels: Vec<(String, String)>,
    /// Bound after which the container is force-removed.
    pub timeout: Duration,
}

/// The container definition plus the password file to upload before start.
pub struct PreparedTransferContainer {
    pub body: bollard::models::ContainerCreateBody,
    /// Contents of the libpq password file, when credentials travel that way.
    pub pgpass: Option<String>,
}

/// One URL with its password removed, and the matching password-file line.
struct SplitCredentials {
    url_without_password: String,
    pgpass_line: Option<String>,
}

/// Escape a password-file field: `:` and `\` are the only special characters.
fn pgpass_escape(field: &str) -> String {
    field.replace('\\', "\\\\").replace(':', "\\:")
}

fn split_postgres_credentials(raw: &str) -> Result<SplitCredentials, String> {
    let mut url =
        url::Url::parse(raw).map_err(|e| format!("not a valid connection URL ({})", e))?;
    let Some(password) = url.password().map(str::to_string) else {
        return Ok(SplitCredentials {
            url_without_password: raw.to_string(),
            pgpass_line: None,
        });
    };
    let decode = |value: &str| {
        percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .map(|v| v.into_owned())
            .map_err(|_| "the URL credentials are not valid UTF-8".to_string())
    };
    let password = decode(&password)?;
    let username = decode(url.username())?;
    let host = url
        .host_str()
        .ok_or_else(|| "the URL has no host".to_string())?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    // libpq compares the port as a string and defaults it to 5432.
    let port = url.port().unwrap_or(5432).to_string();
    url.set_password(None)
        .map_err(|_| "cannot remove the password from the URL".to_string())?;
    Ok(SplitCredentials {
        url_without_password: url.to_string(),
        pgpass_line: Some(format!(
            "{}:{}:*:{}:{}",
            pgpass_escape(&host),
            pgpass_escape(&port),
            pgpass_escape(&username),
            pgpass_escape(&password)
        )),
    })
}

/// Build the container definition for a transfer. Pure, so what reaches
/// Docker (`Config.Env`, `Cmd`) can be checked without a daemon.
pub fn prepare_transfer_container(
    spec: &TransferContainerSpec<'_>,
) -> Result<PreparedTransferContainer, String> {
    use bollard::models::{ContainerCreateBody, HostConfig};

    let (source, destination, pgpass) = match spec.credentials {
        TransferCredentials::InUrl => (
            spec.source_url.to_string(),
            spec.destination_url.to_string(),
            None,
        ),
        TransferCredentials::PgPassFile => {
            let source = split_postgres_credentials(spec.source_url)
                .map_err(|e| format!("source URL: {}", e))?;
            let destination = split_postgres_credentials(spec.destination_url)
                .map_err(|e| format!("destination URL: {}", e))?;
            let lines: Vec<String> = [source.pgpass_line, destination.pgpass_line]
                .into_iter()
                .flatten()
                .collect();
            (
                source.url_without_password,
                destination.url_without_password,
                Some(format!("{}\n", lines.join("\n"))),
            )
        }
    };

    let mut env = vec![format!("SRC={}", source), format!("DST={}", destination)];
    if pgpass.is_some() {
        env.push(format!("PGPASSFILE={}/{}", PGPASS_DIR, PGPASS_FILE));
    }
    let labels = (!spec.labels.is_empty()).then(|| spec.labels.iter().cloned().collect());

    Ok(PreparedTransferContainer {
        body: ContainerCreateBody {
            image: Some(spec.image.to_string()),
            cmd: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                spec.command.to_string(),
            ]),
            env: Some(env),
            labels,
            host_config: Some(HostConfig {
                network_mode: Some(spec.network_mode.to_string()),
                auto_remove: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        },
        pgpass,
    })
}

/// A tar archive holding the password file, mode 0600, owned by root (the
/// user the client images run `sh -c` as).
fn pgpass_archive(contents: &str) -> Result<Vec<u8>, std::io::Error> {
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o600);
    header.set_uid(0);
    header.set_gid(0);
    header.set_cksum();
    let mut builder = tar::Builder::new(Vec::new());
    builder.append_data(&mut header, PGPASS_FILE, contents.as_bytes())?;
    builder.into_inner()
}

/// Run a one-off transfer container to completion.
///
/// The container is always removed: after a clean exit, after a failed exit,
/// and on timeout (force-removal is what cancels a hung transfer).
pub async fn run_transfer_container(
    docker: &Docker,
    spec: &TransferContainerSpec<'_>,
) -> Result<(), DataTransferError> {
    use bollard::query_parameters::{
        CreateContainerOptionsBuilder, CreateImageOptions, StartContainerOptions,
        UploadToContainerOptions,
    };

    let name = format!(
        "{}-{}",
        spec.name_prefix,
        &uuid::Uuid::new_v4().to_string()[..8]
    );
    let prepared =
        prepare_transfer_container(spec).map_err(|reason| DataTransferError::Credentials {
            name: name.clone(),
            reason,
        })?;

    // Ensure the image exists (no-op when already pulled), bounded.
    let pull = async {
        let mut pull = docker.create_image(
            Some(CreateImageOptions {
                from_image: Some(spec.image.to_string()),
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(item) = pull.next().await {
            item.map_err(|e| e.to_string())?;
        }
        Ok::<(), String>(())
    };
    match tokio::time::timeout(IMAGE_PULL_TIMEOUT, pull).await {
        Ok(Ok(())) => {}
        Ok(Err(reason)) => {
            return Err(DataTransferError::ImagePull {
                image: spec.image.to_string(),
                reason,
            })
        }
        Err(_) => {
            return Err(DataTransferError::ImagePull {
                image: spec.image.to_string(),
                reason: format!("timed out after {}s", IMAGE_PULL_TIMEOUT.as_secs()),
            })
        }
    }

    let container = docker
        .create_container(
            Some(CreateContainerOptionsBuilder::new().name(&name).build()),
            prepared.body,
        )
        .await
        .map_err(|e| DataTransferError::ContainerCreate {
            name: name.clone(),
            reason: e.to_string(),
        })?;

    if let Some(contents) = prepared.pgpass.as_deref() {
        let upload = async {
            let archive = pgpass_archive(contents).map_err(|e| e.to_string())?;
            docker
                .upload_to_container(
                    &container.id,
                    Some(UploadToContainerOptions {
                        path: PGPASS_DIR.to_string(),
                        ..Default::default()
                    }),
                    bollard::body_full(bytes::Bytes::from(archive)),
                )
                .await
                .map_err(|e| e.to_string())
        };
        if let Err(reason) = upload.await {
            remove_container_forcefully(docker, &container.id).await;
            return Err(DataTransferError::Credentials { name, reason });
        }
    }

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

/// Force-remove every container carrying `label_key` that was created before
/// `created_before` (unix seconds). Used at startup to stop transfers a
/// previous process started and can no longer await. Returns how many were
/// removed.
pub async fn remove_labelled_containers(
    docker: &Docker,
    label_key: &str,
    created_before: i64,
) -> Result<usize, String> {
    use bollard::query_parameters::ListContainersOptionsBuilder;

    let filters = std::collections::HashMap::from([("label", vec![label_key])]);
    let containers = docker
        .list_containers(Some(
            ListContainersOptionsBuilder::new()
                .all(true)
                .filters(&filters)
                .build(),
        ))
        .await
        .map_err(|e| e.to_string())?;
    let mut removed = 0;
    for container in containers {
        let (Some(id), Some(created)) = (container.id, container.created) else {
            continue;
        };
        if created < created_before {
            remove_container_forcefully(docker, &id).await;
            removed += 1;
        }
    }
    Ok(removed)
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

pub async fn remove_container_forcefully(docker: &Docker, container_id: &str) {
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
            format!(
                "for i in $(seq 1 45); do pg_isready -d \"$DST\" >/dev/null 2>&1 && break; sleep 2; done; {}",
                POSTGRES_COPY_PIPELINE
            ),
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

/// Shell pipeline that copies a PostgreSQL database into another one, all or
/// nothing. Shared by the importer and by populate.
///
/// The base mechanism is `pg_dump --no-owner --no-privileges "$SRC" | psql
/// "$DST"`; three things make it trustworthy:
///
/// - the whole script runs inside one transaction whose `COMMIT` is written
///   **only after `pg_dump` exits 0** (`{ echo BEGIN; pg_dump && echo
///   COMMIT; }`). When `pg_dump` dies mid-way (source connection killed,
///   network cut), `psql` sees a clean EOF with the transaction still open,
///   and the server rolls it back at disconnect. `psql --single-transaction`
///   is NOT enough: it commits at EOF no matter why the input ended, which
///   leaves a half-copied database behind;
/// - `set -o pipefail` — the pipeline's status would otherwise be `psql`'s
///   alone, so a `pg_dump` that cannot even authenticate would end as a
///   success with nothing copied;
/// - `ON_ERROR_STOP=1` — `psql` otherwise carries on (and exits 0) after an
///   SQL error.
///
/// `-X` ignores any `psqlrc` in the image. Credentials come from
/// [`TransferCredentials::PgPassFile`], never from the command.
pub const POSTGRES_COPY_PIPELINE: &str = "set -o pipefail; { echo 'BEGIN;'; pg_dump --no-owner --no-privileges \"$SRC\" && echo 'COMMIT;'; } | psql -X -q -v ON_ERROR_STOP=1 \"$DST\" >/dev/null";

/// The populate command: [`POSTGRES_COPY_PIPELINE`] into an existing,
/// already-running destination (no readiness wait needed).
pub fn postgres_populate_command() -> &'static str {
    POSTGRES_COPY_PIPELINE
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
    fn importer_postgres_command_waits_then_copies_atomically() {
        let (image, command) = dump_restore_command("postgres").expect("postgres command");
        assert_eq!(image, "postgres:16-alpine");
        assert!(command.starts_with("for i in $(seq 1 45); do pg_isready"));
        assert!(command.ends_with(POSTGRES_COPY_PIPELINE));
    }

    #[test]
    fn copy_pipeline_commits_only_after_pg_dump_succeeds() {
        let command = postgres_populate_command();
        assert!(command.starts_with("set -o pipefail;"));
        assert!(command.contains(
            "{ echo 'BEGIN;'; pg_dump --no-owner --no-privileges \"$SRC\" && echo 'COMMIT;'; }"
        ));
        assert!(command.contains("-v ON_ERROR_STOP=1"));
        // --single-transaction commits at EOF even when pg_dump died.
        assert!(!command.contains("--single-transaction"));
        assert!(command.contains("\"$DST\""));
        assert!(!command.contains("postgres://"));
    }

    fn spec<'a>(source: &'a str, destination: &'a str) -> TransferContainerSpec<'a> {
        TransferContainerSpec {
            image: "postgres:18-alpine",
            command: POSTGRES_COPY_PIPELINE,
            source_url: source,
            destination_url: destination,
            credentials: TransferCredentials::PgPassFile,
            network_mode: "host",
            name_prefix: "temps-populate",
            labels: vec![("sh.temps.populate_run".to_string(), "7".to_string())],
            timeout: Duration::from_secs(60),
        }
    }

    #[test]
    fn passwords_stay_out_of_env_and_command() {
        let source = "postgres://app:SRC%3Asecret%40x@db.example.com:6543/app?sslmode=require";
        let destination = "postgres://svc:DST-secret@127.0.0.1:5433/my_app?sslmode=disable";
        let prepared = prepare_transfer_container(&spec(source, destination)).expect("prepare");

        let env = prepared.body.env.clone().expect("env");
        let cmd = prepared.body.cmd.clone().expect("cmd").join(" ");
        for secret in ["SRC%3Asecret%40x", "SRC:secret@x", "DST-secret"] {
            assert!(!env.iter().any(|e| e.contains(secret)), "{:?}", env);
            assert!(!cmd.contains(secret), "{}", cmd);
        }
        assert!(
            env.contains(&"SRC=postgres://app@db.example.com:6543/app?sslmode=require".to_string())
        );
        assert!(
            env.contains(&"DST=postgres://svc@127.0.0.1:5433/my_app?sslmode=disable".to_string())
        );
        assert!(env.contains(&format!("PGPASSFILE={}/{}", PGPASS_DIR, PGPASS_FILE)));

        let pgpass = prepared.pgpass.expect("pgpass");
        assert_eq!(
            pgpass,
            "db.example.com:6543:*:app:SRC\\:secret@x\n127.0.0.1:5433:*:svc:DST-secret\n"
        );
        assert_eq!(
            prepared
                .body
                .labels
                .expect("labels")
                .get("sh.temps.populate_run"),
            Some(&"7".to_string())
        );
    }

    #[test]
    fn pgpass_defaults_the_port_and_skips_passwordless_urls() {
        let prepared = prepare_transfer_container(&spec(
            "postgres://app:pw@db.example.com/app",
            "postgres://svc@127.0.0.1:5433/my_app",
        ))
        .expect("prepare");
        assert_eq!(
            prepared.pgpass.as_deref(),
            Some("db.example.com:5432:*:app:pw\n")
        );
    }

    #[test]
    fn pgpass_archive_is_owner_only() {
        let archive = pgpass_archive("h:5432:*:u:p\n").expect("archive");
        let mut reader = tar::Archive::new(archive.as_slice());
        let entry = reader
            .entries()
            .expect("entries")
            .next()
            .expect("one entry")
            .expect("entry");
        assert_eq!(entry.header().mode().expect("mode"), 0o600);
        assert_eq!(entry.path().expect("path").to_string_lossy(), PGPASS_FILE);
    }

    #[test]
    fn in_url_credentials_are_passed_through() {
        let mut spec = spec("mysql://u:p@h/db", "mysql://u:q@h2/db");
        spec.credentials = TransferCredentials::InUrl;
        let prepared = prepare_transfer_container(&spec).expect("prepare");
        assert!(prepared.pgpass.is_none());
        assert_eq!(
            prepared.body.env.expect("env"),
            vec![
                "SRC=mysql://u:p@h/db".to_string(),
                "DST=mysql://u:q@h2/db".to_string()
            ]
        );
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
