// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Populate a database of an existing managed PostgreSQL service from an
//! external PostgreSQL ("populate").
//!
//! The importer copies data only into services it has just created, into a
//! database named after the source. This operation targets a service that
//! already exists and a database the caller names — typically the
//! per-environment database a project link provisions
//! (`<project>_<environment>`), so a deployment that runs afterwards finds the
//! database already there and simply uses it.
//!
//! Flow:
//!
//! 1. [`ServicePopulateService::start`] validates everything that can be
//!    validated up front — destination name, source URL (SSRF), service type
//!    and topology, that the service is not the control plane's own database
//!    server, whether the destination is empty or (for `replace`) still has
//!    sessions — inserts a `service_populate_runs` row in `running` and
//!    spawns the copy. The HTTP request returns immediately with the run.
//! 2. The background task never writes a half-copied database under the
//!    final name:
//!    - a destination that does not exist yet, or one being replaced, is
//!      copied into a staging database `<db>__populate_<run_id>` created as
//!      the service user (exactly like the provider's provisioning, so the
//!      user owns it). Only after the copy succeeded is the old destination
//!      dropped and the staging database renamed to the final name; on
//!      failure the staging database is dropped and the destination is
//!      untouched;
//!    - an existing, empty destination is copied into directly — the copy
//!      itself is all-or-nothing (see
//!      [`crate::data_transfer::POSTGRES_COPY_PIPELINE`]: one transaction
//!      whose `COMMIT` is only sent after `pg_dump` exited 0), so a failure
//!      leaves it empty.
//!
//!    The copy runs in the shared transfer container, bounded by
//!    [`DATA_TRANSFER_TIMEOUT`]; credentials reach it through a password
//!    file, never its environment or command line.
//!
//! The source URL is never persisted, logged or returned: the row keeps a
//! copy with user and password masked, and every error message is scrubbed
//! of URLs, passwords and row data before it is stored.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::FutureExt;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder,
};
use sqlx::Connection;
use temps_core::{DockerHandle, DockerUnavailable};
use temps_entities::service_populate_runs;
use thiserror::Error;
use tracing::{error, info, warn};

use crate::data_transfer::{
    percent_encode_userinfo, postgres_client_image, postgres_populate_command,
    remove_labelled_containers, run_transfer_container, TransferContainerSpec, TransferCredentials,
    DATA_TRANSFER_TIMEOUT,
};
use crate::externalsvc::postgres::{PostgresConfig, PostgresInputConfig};
use crate::services::{ExternalServiceError, ExternalServiceManager};

/// Run status: the copy is in progress.
pub const POPULATE_STATUS_RUNNING: &str = "running";
/// Run status: the copy finished and the destination holds the source data.
pub const POPULATE_STATUS_COMPLETED: &str = "completed";
/// Run status: the copy failed; `error_message` says why.
pub const POPULATE_STATUS_FAILED: &str = "failed";

/// Databases that exist on every PostgreSQL server and must never be
/// dropped or overwritten.
const RESERVED_DATABASES: [&str; 3] = ["postgres", "template0", "template1"];

/// Query parameters a source URL may carry. Everything else is refused: libpq
/// honours `host`/`hostaddr`/`port`/`dbname` in the query string, so an
/// unrestricted query would let a URL whose authority passed the SSRF check
/// connect somewhere else entirely.
const ALLOWED_SOURCE_QUERY_PARAMS: [&str; 4] = [
    "sslmode",
    "sslrootcert",
    "connect_timeout",
    "application_name",
];

const ALLOWED_SSLMODES: [&str; 6] = [
    "disable",
    "allow",
    "prefer",
    "require",
    "verify-ca",
    "verify-full",
];

/// Bound on the control-plane SQL (existence checks, CREATE/DROP DATABASE).
const ADMIN_SQL_TIMEOUT: Duration = Duration::from_secs(60);

/// Separator between the destination name and the run id in the staging
/// database name.
const STAGING_SEPARATOR: &str = "__populate_";

/// Docker label on every populate transfer container; its value is the run
/// id. A restarted control plane finds and removes these.
pub fn populate_container_label() -> String {
    format!("{}populate_run", temps_core::DOCKER_LABEL_PREFIX)
}

#[derive(Debug, Error)]
pub enum ServicePopulateError {
    #[error("Service {service_id} not found")]
    ServiceNotFound { service_id: i32 },

    #[error("Service {service_id} cannot be populated: {reason}")]
    UnsupportedService { service_id: i32, reason: String },

    #[error(
        "Service {service_id} runs on {host}:{port}, the database server of the Temps control \
         plane itself; populating it is refused"
    )]
    ControlPlaneDatabase {
        service_id: i32,
        host: String,
        port: u16,
    },

    #[error("Invalid destination database '{database}': {reason}")]
    InvalidDatabaseName { database: String, reason: String },

    #[error("Invalid source URL: {reason}")]
    InvalidSourceUrl { reason: String },

    #[error(
        "Database '{database}' of service {service_id} is not empty ({table_count} table(s) in \
         schema public); pass replace=true to drop and recreate it before copying"
    )]
    TargetNotEmpty {
        service_id: i32,
        database: String,
        table_count: i64,
    },

    #[error(
        "Database '{database}' of service {service_id} has {sessions} open session(s); stop the \
         clients or pass disconnect_clients=true to terminate them when the copy is swapped in"
    )]
    TargetInUse {
        service_id: i32,
        database: String,
        sessions: i64,
    },

    #[error(
        "Database '{database}' of service {service_id} was created while the copy was running; \
         the copy was discarded — run populate again"
    )]
    TargetAppeared { service_id: i32, database: String },

    #[error("A populate of database '{database}' of service {service_id} is already running")]
    AlreadyRunning { service_id: i32, database: String },

    #[error("Populate run {run_id} not found for service {service_id}")]
    RunNotFound { service_id: i32, run_id: i32 },

    #[error("Service {service_id} runs no local workloads on this process: {reason}")]
    LocalWorkloadsDisabled { service_id: i32, reason: String },

    #[error(transparent)]
    DockerUnavailable(#[from] DockerUnavailable),

    #[error("Cannot reach database server of service {service_id}: {reason}")]
    TargetConnection { service_id: i32, reason: String },

    #[error("Failed to read configuration of service {service_id}: {reason}")]
    ServiceConfig { service_id: i32, reason: String },

    #[error("Database error while handling populate runs: {0}")]
    Database(#[from] sea_orm::DbErr),
}

/// What the caller asked for.
#[derive(Debug, Clone)]
pub struct StartPopulateRequest {
    pub service_id: i32,
    pub database: String,
    pub source_url: String,
    pub replace: bool,
    /// With `replace`: terminate the destination's sessions instead of
    /// refusing when it has any.
    pub disconnect_clients: bool,
    pub created_by: Option<i32>,
}

/// What has to happen to the destination database around the copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPreparation {
    /// The database does not exist yet: copy into a staging database and
    /// rename it to the final name on success.
    Create,
    /// The database exists and has no tables in `public`: copy into it (the
    /// copy is one transaction, so a failure leaves it empty).
    UseExisting,
    /// The database exists and `replace` was requested: copy into a staging
    /// database; on success drop the old one and rename the staging one.
    Recreate,
}

/// Decide how to prepare the destination, refusing to copy into a database
/// that already holds tables unless the caller explicitly asked to replace it.
pub fn plan_target_preparation(
    service_id: i32,
    database: &str,
    exists: bool,
    table_count: i64,
    replace: bool,
) -> Result<TargetPreparation, ServicePopulateError> {
    match (exists, replace) {
        (false, _) => Ok(TargetPreparation::Create),
        (true, true) => Ok(TargetPreparation::Recreate),
        (true, false) if table_count == 0 => Ok(TargetPreparation::UseExisting),
        (true, false) => Err(ServicePopulateError::TargetNotEmpty {
            service_id,
            database: database.to_string(),
            table_count,
        }),
    }
}

/// Staging database of a run: `<database>__populate_<run_id>`, with the
/// destination part shortened when needed so the whole name fits in
/// PostgreSQL's 63-byte identifiers (a longer one would be silently
/// truncated, and two runs could collide). `database` is ASCII (it passed
/// [`validate_database_name`]), so byte slicing is safe.
pub fn staging_database_name(database: &str, run_id: i32) -> String {
    let suffix = format!("{}{}", STAGING_SEPARATOR, run_id);
    let keep = database.len().min(63usize.saturating_sub(suffix.len()));
    format!("{}{}", &database[..keep], suffix)
}

/// Validate the destination database name with the same identifier rule the
/// project-link provisioning uses for custom names (`[a-z_][a-z0-9_]{0,62}`),
/// and refuse the server's own databases and names that look like a staging
/// database.
pub fn validate_database_name(database: &str) -> Result<(), ServicePopulateError> {
    if !crate::services::is_valid_custom_database_name(database) {
        return Err(ServicePopulateError::InvalidDatabaseName {
            database: database.to_string(),
            reason: "must match [a-z_][a-z0-9_]{0,62}".to_string(),
        });
    }
    if RESERVED_DATABASES.contains(&database) {
        return Err(ServicePopulateError::InvalidDatabaseName {
            database: database.to_string(),
            reason: "is a PostgreSQL system database".to_string(),
        });
    }
    if database.contains(STAGING_SEPARATOR) {
        return Err(ServicePopulateError::InvalidDatabaseName {
            database: database.to_string(),
            reason: format!("'{}' is reserved for staging databases", STAGING_SEPARATOR),
        });
    }
    Ok(())
}

/// Host and port of a PostgreSQL server, to recognise the control plane's own
/// database server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseEndpoint {
    pub host: String,
    pub port: u16,
}

impl DatabaseEndpoint {
    /// From a `postgres://` URL (the control plane's `DATABASE_URL`).
    pub fn from_url(raw: &str) -> Option<Self> {
        let url = url::Url::parse(raw).ok()?;
        if !matches!(url.scheme(), "postgres" | "postgresql") {
            return None;
        }
        Some(Self {
            host: url.host_str()?.to_string(),
            port: url.port().unwrap_or(5432),
        })
    }

    /// Every spelling of "this machine" compares equal.
    fn normalized_host(&self) -> String {
        let host = self
            .host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();
        let is_local = host == "localhost"
            || host == "0.0.0.0"
            || host == "::"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if is_local {
            "local".to_string()
        } else {
            host
        }
    }

    pub fn same_server(&self, other: &Self) -> bool {
        self.port == other.port && self.normalized_host() == other.normalized_host()
    }
}

/// Shape checks on a source URL that need no network: scheme, database path
/// and the query-parameter allowlist. [`validate_source_url`] adds the SSRF
/// check on top.
fn validate_source_url_shape(raw: &str) -> Result<(), ServicePopulateError> {
    let invalid = |reason: String| ServicePopulateError::InvalidSourceUrl { reason };
    // `redact_url_password` keeps a parse failure's input as-is, so never echo
    // the raw string — only describe what is wrong with it.
    let parsed =
        url::Url::parse(raw).map_err(|e| invalid(format!("not a valid connection URL ({})", e)))?;

    if !matches!(parsed.scheme(), "postgres" | "postgresql") {
        return Err(invalid(format!(
            "scheme '{}' is not supported; use postgres:// or postgresql://",
            parsed.scheme()
        )));
    }

    let database = percent_encoding::percent_decode_str(parsed.path().trim_start_matches('/'))
        .decode_utf8()
        .map_err(|_| invalid("the database name is not valid UTF-8".to_string()))?;
    if database.is_empty() {
        return Err(invalid(
            "the URL must name the source database (postgres://user:pass@host:port/<database>)"
                .to_string(),
        ));
    }
    let database_is_plain = database.len() <= 63
        && database
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if !database_is_plain {
        return Err(invalid(format!(
            "source database name '{}' may only contain letters, digits, '_', '-' and '.'",
            database
        )));
    }

    for (key, value) in parsed.query_pairs() {
        if !ALLOWED_SOURCE_QUERY_PARAMS.contains(&key.as_ref()) {
            return Err(invalid(format!(
                "query parameter '{}' is not allowed; only {} are accepted",
                key,
                ALLOWED_SOURCE_QUERY_PARAMS.join(", ")
            )));
        }
        let value_ok = match key.as_ref() {
            "sslmode" => ALLOWED_SSLMODES.contains(&value.as_ref()),
            // Only the system CA store: a file path would name a file inside
            // the transfer container, which holds none.
            "sslrootcert" => value == "system",
            "connect_timeout" => !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()),
            _ => value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')),
        };
        if !value_ok {
            return Err(invalid(format!(
                "query parameter '{}' has an unsupported value '{}'",
                key, value
            )));
        }
    }
    Ok(())
}

/// Full source URL validation: shape, then the same SSRF guard the importer
/// applies (`validate_external_database_url_async` — public address, host
/// resolved and every resolved address checked).
pub async fn validate_source_url(raw: &str) -> Result<(), ServicePopulateError> {
    validate_source_url_shape(raw)?;
    temps_core::url_validation::validate_external_database_url_async(raw)
        .await
        .map_err(|e| ServicePopulateError::InvalidSourceUrl {
            reason: format!(
                "{} — the source must be a publicly reachable PostgreSQL server",
                e
            ),
        })?;
    Ok(())
}

/// The source URL as it may be stored and shown: user and password replaced
/// by `***`, host, port, database and query kept.
pub fn mask_source_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(_) => temps_core::url_validation::redact_url_password(raw),
        // Unparseable input is never echoed back.
        Err(_) => "***".to_string(),
    }
}

/// Remove every secret from a message before it is logged or stored. Covers
/// the raw URLs and the passwords both as typed and percent-decoded, since
/// either form may appear in a client tool's error output.
fn scrub_secrets(message: &str, secrets: &[&str]) -> String {
    let mut scrubbed = message.to_string();
    let mut needles: Vec<String> = Vec::new();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        needles.push(secret.to_string());
        if let Ok(decoded) = percent_encoding::percent_decode_str(secret).decode_utf8() {
            needles.push(decoded.into_owned());
        }
    }
    // Longest first, so a URL is replaced whole before its password is.
    needles.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for needle in needles.iter().filter(|n| !n.is_empty()) {
        scrubbed = scrubbed.replace(needle.as_str(), "***");
    }
    scrubbed
}

/// Drop the parts of client output that carry row data: PostgreSQL's
/// `CONTEXT:` lines quote the failing `COPY` input line, and `DETAIL:` lines
/// quote key values. The `ERROR:` line that names the problem stays.
fn strip_row_data(message: &str) -> String {
    message
        .lines()
        .filter(|line| !line.contains("CONTEXT:") && !line.contains("DETAIL:"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Everything the background copy needs about the destination server.
#[derive(Clone)]
struct TargetServer {
    service_id: i32,
    /// The provider's `host`/`port` parameters: the port published on the
    /// host. The transfer container (host network) uses them, and so does the
    /// check against the control plane's own database server.
    host: String,
    port: String,
    /// Where the control plane opens its SQL connections, from
    /// `ExternalServiceManager::get_service_admin_endpoint`: the container
    /// name and internal port when the control plane runs in a container,
    /// `host`/`port` otherwise.
    admin_host: String,
    admin_port: String,
    username: String,
    password: String,
    /// Major version, known after [`Self::inspect`]; `DROP ... WITH (FORCE)`
    /// needs 13+.
    major_version: u32,
}

impl TargetServer {
    fn url_for(&self, host: &str, port: &str, database: &str) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode=disable",
            percent_encode_userinfo(&self.username),
            percent_encode_userinfo(&self.password),
            host,
            port,
            database
        )
    }

    /// Connection URL for control-plane SQL, the way the provider's own
    /// `create_database` connects.
    fn admin_url(&self, database: &str) -> String {
        self.url_for(&self.admin_host, &self.admin_port, database)
    }

    /// Connection URL the transfer container uses. It runs on the host
    /// network, where published service ports bind IPv4 only; "localhost"
    /// resolves to ::1 first inside the container and would be refused.
    fn transfer_url(&self, database: &str) -> String {
        let host = if self.host == "localhost" {
            "127.0.0.1"
        } else {
            self.host.as_str()
        };
        self.url_for(host, &self.port, database)
    }

    fn endpoint(&self) -> Option<DatabaseEndpoint> {
        Some(DatabaseEndpoint {
            host: self.host.clone(),
            port: self.port.parse().ok()?,
        })
    }

    async fn connect(&self, database: &str) -> Result<sqlx::PgConnection, ServicePopulateError> {
        let url = self.admin_url(database);
        tokio::time::timeout(ADMIN_SQL_TIMEOUT, sqlx::PgConnection::connect(&url))
            .await
            .map_err(|_| ServicePopulateError::TargetConnection {
                service_id: self.service_id,
                reason: format!(
                    "connecting to database '{}' timed out after {}s",
                    database,
                    ADMIN_SQL_TIMEOUT.as_secs()
                ),
            })?
            .map_err(|e| ServicePopulateError::TargetConnection {
                service_id: self.service_id,
                reason: scrub_secrets(
                    &format!("connecting to database '{}': {}", database, e),
                    &[&url, &self.password],
                ),
            })
    }

    fn sql_error(&self, what: &str, e: sqlx::Error) -> ServicePopulateError {
        ServicePopulateError::TargetConnection {
            service_id: self.service_id,
            reason: scrub_secrets(&format!("{}: {}", what, e), &[&self.password]),
        }
    }

    /// Run one statement against the maintenance database.
    async fn admin_execute(&self, sql: &str, what: &str) -> Result<(), ServicePopulateError> {
        let mut admin = self.connect("postgres").await?;
        let result = sqlx::query(sql).execute(&mut admin).await;
        let _ = admin.close().await;
        result.map(|_| ()).map_err(|e| self.sql_error(what, e))
    }

    /// Major version, existence and table count of `database`.
    async fn inspect(&self, database: &str) -> Result<TargetState, ServicePopulateError> {
        let mut admin = self.connect("postgres").await?;
        let version_num: i32 =
            sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
                .fetch_one(&mut admin)
                .await
                .map_err(|e| self.sql_error("reading server version", e))?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
                .bind(database)
                .fetch_one(&mut admin)
                .await
                .map_err(|e| self.sql_error("checking database existence", e))?;
        let _ = admin.close().await;

        let table_count = if exists {
            let mut conn = self.connect(database).await?;
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_catalog.pg_class c \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p')",
            )
            .fetch_one(&mut conn)
            .await
            .map_err(|e| self.sql_error("counting tables", e))?;
            let _ = conn.close().await;
            count
        } else {
            0
        };

        Ok(TargetState {
            major_version: (version_num / 10_000) as u32,
            exists,
            table_count,
        })
    }

    async fn database_exists(&self, database: &str) -> Result<bool, ServicePopulateError> {
        let mut admin = self.connect("postgres").await?;
        let exists: Result<bool, _> =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
                .bind(database)
                .fetch_one(&mut admin)
                .await;
        let _ = admin.close().await;
        exists.map_err(|e| self.sql_error("checking database existence", e))
    }

    /// Sessions connected to `database`, other than this one.
    async fn active_sessions(&self, database: &str) -> Result<i64, ServicePopulateError> {
        let mut admin = self.connect("postgres").await?;
        let count: Result<i64, _> = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE datname = $1 AND pid <> pg_backend_pid()",
        )
        .bind(database)
        .fetch_one(&mut admin)
        .await;
        let _ = admin.close().await;
        count.map_err(|e| self.sql_error("counting sessions", e))
    }

    // Every name below passed validate_database_name ([a-z_][a-z0-9_]*) or
    // is a staging name built from one, so the quoted identifiers cannot be
    // broken out of.

    /// `CREATE DATABASE` as the service user — exactly what the provider's
    /// provisioning does — so the user owns it.
    async fn create_database(&self, database: &str) -> Result<(), ServicePopulateError> {
        self.admin_execute(
            &format!("CREATE DATABASE \"{}\"", database),
            &format!("creating database '{}'", database),
        )
        .await
    }

    /// `force` terminates the sessions still attached (PG13+).
    async fn drop_database(&self, database: &str, force: bool) -> Result<(), ServicePopulateError> {
        let sql = if force && self.major_version >= 13 {
            format!("DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)", database)
        } else {
            format!("DROP DATABASE IF EXISTS \"{}\"", database)
        };
        self.admin_execute(&sql, &format!("dropping database '{}'", database))
            .await
    }

    async fn rename_database(&self, from: &str, to: &str) -> Result<(), ServicePopulateError> {
        self.admin_execute(
            &format!("ALTER DATABASE \"{}\" RENAME TO \"{}\"", from, to),
            &format!("renaming database '{}' to '{}'", from, to),
        )
        .await
    }

    async fn database_size(&self, database: &str) -> Result<i64, ServicePopulateError> {
        let mut admin = self.connect("postgres").await?;
        let size: Result<i64, _> = sqlx::query_scalar("SELECT pg_database_size($1)")
            .bind(database)
            .fetch_one(&mut admin)
            .await;
        let _ = admin.close().await;
        size.map_err(|e| self.sql_error("measuring database size", e))
    }
}

#[derive(Debug, Clone, Copy)]
struct TargetState {
    major_version: u32,
    exists: bool,
    table_count: i64,
}

/// Copy into the destination without ever leaving a half-copied database
/// under its final name. `copy` receives the database to copy into.
///
/// - [`TargetPreparation::UseExisting`]: straight into the (empty)
///   destination; the copy is one transaction.
/// - [`TargetPreparation::Create`] / [`TargetPreparation::Recreate`]: into
///   `staging`. On success the old destination (if any) is dropped and
///   `staging` renamed to the final name; on failure `staging` is dropped and
///   the destination is left as it was.
async fn copy_into_destination<F, Fut>(
    target: &TargetServer,
    database: &str,
    staging: &str,
    preparation: TargetPreparation,
    disconnect_clients: bool,
    copy: F,
) -> Result<(), String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    if preparation == TargetPreparation::UseExisting {
        return copy(database.to_string()).await;
    }

    // A leftover from an interrupted run with the same id cannot exist (ids
    // are not reused), but a stale one must never be renamed into place.
    target
        .drop_database(staging, true)
        .await
        .map_err(|e| e.to_string())?;
    target
        .create_database(staging)
        .await
        .map_err(|e| e.to_string())?;

    let discard_staging = || async {
        if let Err(e) = target.drop_database(staging, true).await {
            warn!(
                service_id = target.service_id,
                staging,
                error = %e,
                "Could not drop the staging database of a failed populate; drop it by hand"
            );
        }
    };

    if let Err(e) = copy(staging.to_string()).await {
        discard_staging().await;
        return Err(e);
    }

    let make_room = async {
        if preparation == TargetPreparation::Recreate {
            if !disconnect_clients {
                let sessions = target.active_sessions(database).await?;
                if sessions > 0 {
                    return Err(ServicePopulateError::TargetInUse {
                        service_id: target.service_id,
                        database: database.to_string(),
                        sessions,
                    });
                }
            }
            // Without disconnect_clients a plain DROP refuses a session that
            // connected after the check above, instead of cutting it off.
            target.drop_database(database, disconnect_clients).await
        } else if target.database_exists(database).await? {
            Err(ServicePopulateError::TargetAppeared {
                service_id: target.service_id,
                database: database.to_string(),
            })
        } else {
            Ok(())
        }
    };
    if let Err(e) = make_room.await {
        discard_staging().await;
        return Err(e.to_string());
    }

    // The old destination is gone; the copy now only exists in `staging`, so
    // a failed rename must keep it.
    target
        .rename_database(staging, database)
        .await
        .map_err(|e| {
            format!(
                "{} — the copied data is kept in database '{}'; rename it to '{}' by hand",
                e, staging, database
            )
        })
}

/// Starts populate runs and answers questions about them.
pub struct ServicePopulateService {
    db: Arc<DatabaseConnection>,
    external_services: Arc<ExternalServiceManager>,
    docker: Arc<DockerHandle>,
    /// The control plane's own database server, which populate must never
    /// write to. `None` when the configuration does not name one.
    control_plane_database: Option<DatabaseEndpoint>,
}

impl ServicePopulateService {
    pub fn new(
        db: Arc<DatabaseConnection>,
        external_services: Arc<ExternalServiceManager>,
        docker: Arc<DockerHandle>,
        control_plane_database: Option<DatabaseEndpoint>,
    ) -> Self {
        Self {
            db,
            external_services,
            docker,
            control_plane_database,
        }
    }

    /// Validate the request, record a `running` run and start the copy in the
    /// background. Returns the run so the caller can poll it.
    pub async fn start(
        &self,
        request: StartPopulateRequest,
    ) -> Result<service_populate_runs::Model, ServicePopulateError> {
        let service_id = request.service_id;
        validate_database_name(&request.database)?;
        validate_source_url(&request.source_url).await?;

        let mut target = self.resolve_target(service_id).await?;
        if let (Some(control_plane), Some(endpoint)) =
            (&self.control_plane_database, target.endpoint())
        {
            if control_plane.same_server(&endpoint) {
                return Err(ServicePopulateError::ControlPlaneDatabase {
                    service_id,
                    host: endpoint.host,
                    port: endpoint.port,
                });
            }
        }
        // The copy always runs a container: fail before recording anything
        // when this process cannot run one.
        let docker = self.docker.require()?;

        let state = target.inspect(&request.database).await?;
        target.major_version = state.major_version;
        let preparation = plan_target_preparation(
            service_id,
            &request.database,
            state.exists,
            state.table_count,
            request.replace,
        )?;
        if preparation == TargetPreparation::Recreate && !request.disconnect_clients {
            let sessions = target.active_sessions(&request.database).await?;
            if sessions > 0 {
                return Err(ServicePopulateError::TargetInUse {
                    service_id,
                    database: request.database.clone(),
                    sessions,
                });
            }
        }
        let client_image = postgres_client_image(state.major_version);

        // Pre-flight for a readable 409; the partial unique index is the
        // authoritative lock against a concurrent insert.
        if self
            .find_running(service_id, &request.database)
            .await?
            .is_some()
        {
            return Err(ServicePopulateError::AlreadyRunning {
                service_id,
                database: request.database.clone(),
            });
        }

        let now = Utc::now();
        let run = service_populate_runs::ActiveModel {
            service_id: Set(service_id),
            database_name: Set(request.database.clone()),
            source_url_masked: Set(mask_source_url(&request.source_url)),
            replace_existing: Set(request.replace),
            disconnect_clients: Set(request.disconnect_clients),
            status: Set(POPULATE_STATUS_RUNNING.to_string()),
            client_image: Set(client_image.clone()),
            started_at: Set(now),
            created_by: Set(request.created_by),
            ..Default::default()
        }
        .insert(self.db.as_ref())
        .await
        .map_err(|e| match e.sql_err() {
            Some(sea_orm::SqlErr::UniqueConstraintViolation(_)) => {
                ServicePopulateError::AlreadyRunning {
                    service_id,
                    database: request.database.clone(),
                }
            }
            _ => ServicePopulateError::Database(e),
        })?;

        info!(
            run_id = run.id,
            service_id,
            database = %run.database_name,
            source = %run.source_url_masked,
            replace = run.replace_existing,
            disconnect_clients = run.disconnect_clients,
            client_image = %run.client_image,
            "Starting populate of a managed PostgreSQL database"
        );

        let job = PopulateJob {
            run_id: run.id,
            database: request.database,
            source_url: request.source_url,
            preparation,
            disconnect_clients: request.disconnect_clients,
            client_image,
            target,
        };
        let db = self.db.clone();
        tokio::spawn(async move {
            let run_id = job.run_id;
            let outcome = std::panic::AssertUnwindSafe(job.execute(&docker))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| Err("the populate task panicked".to_string()));
            finalize_run(&db, run_id, outcome).await;
        });

        Ok(run)
    }

    /// Runs of a service, newest first. `page` is 1-based; `page_size`
    /// defaults to 20 and is capped at 100.
    pub async fn list_runs(
        &self,
        service_id: i32,
        page: Option<u64>,
        page_size: Option<u64>,
    ) -> Result<(Vec<service_populate_runs::Model>, u64), ServicePopulateError> {
        self.external_services
            .get_service(service_id)
            .await
            .map_err(|e| self.map_service_error(service_id, e))?;
        let page = page.unwrap_or(1).max(1);
        let page_size = page_size.unwrap_or(20).clamp(1, 100);
        let paginator = service_populate_runs::Entity::find()
            .filter(service_populate_runs::Column::ServiceId.eq(service_id))
            .order_by_desc(service_populate_runs::Column::CreatedAt)
            .order_by_desc(service_populate_runs::Column::Id)
            .paginate(self.db.as_ref(), page_size);
        let total = paginator.num_items().await?;
        let items = paginator.fetch_page(page - 1).await?;
        Ok((items, total))
    }

    /// One run, scoped to its service so an id from another service is a 404.
    pub async fn get_run(
        &self,
        service_id: i32,
        run_id: i32,
    ) -> Result<service_populate_runs::Model, ServicePopulateError> {
        service_populate_runs::Entity::find_by_id(run_id)
            .filter(service_populate_runs::Column::ServiceId.eq(service_id))
            .one(self.db.as_ref())
            .await?
            .ok_or(ServicePopulateError::RunNotFound { service_id, run_id })
    }

    /// Clean up after a previous process, at startup.
    ///
    /// A run's copy lives in a task of the process that started it, and its
    /// transfer container is NOT removed when that process dies — Docker
    /// keeps it running, still writing into the destination, with nobody
    /// left to await it. So, in this order:
    ///
    /// 1. force-remove every populate transfer container created before
    ///    `process_started_at`;
    /// 2. mark the runs still `running` that started before
    ///    `process_started_at` as failed.
    ///
    /// Both are bounded by `process_started_at` so a run this process starts
    /// meanwhile is never touched. A staging database left by an interrupted
    /// run is kept (it may hold the only copy if the process died between
    /// dropping the old destination and renaming); the error message names
    /// it.
    pub async fn recover_interrupted_runs(
        &self,
        process_started_at: DateTime<Utc>,
    ) -> Result<(usize, u64), ServicePopulateError> {
        let removed = match self.docker.require() {
            Ok(docker) => {
                match remove_labelled_containers(
                    &docker,
                    &populate_container_label(),
                    process_started_at.timestamp(),
                )
                .await
                {
                    Ok(removed) => removed,
                    Err(reason) => {
                        warn!(
                            reason = %reason,
                            "Could not remove populate transfer containers left by a previous \
                             process; marking their runs failed anyway"
                        );
                        0
                    }
                }
            }
            Err(_) => 0,
        };

        let interrupted = service_populate_runs::Entity::find()
            .filter(service_populate_runs::Column::Status.eq(POPULATE_STATUS_RUNNING))
            .filter(service_populate_runs::Column::StartedAt.lt(process_started_at))
            .all(self.db.as_ref())
            .await?;
        let mut marked = 0;
        for run in interrupted {
            let staging = staging_database_name(&run.database_name, run.id);
            let update = service_populate_runs::ActiveModel {
                id: Set(run.id),
                status: Set(POPULATE_STATUS_FAILED.to_string()),
                error_message: Set(Some(format!(
                    "interrupted: the Temps server restarted while the copy was running and its \
                     transfer container was removed. Database '{}' is as it was before unless \
                     the restart hit the final swap; a staging database '{}' may be left \
                     behind — drop it, or rename it to '{}' if '{}' is missing. Run populate \
                     again.",
                    run.database_name, staging, run.database_name, run.database_name
                ))),
                finished_at: Set(Some(Utc::now())),
                ..Default::default()
            }
            .update(self.db.as_ref())
            .await;
            match update {
                Ok(_) => marked += 1,
                Err(e) => error!(run_id = run.id, error = %e, "Could not mark populate run failed"),
            }
        }
        Ok((removed, marked))
    }

    async fn find_running(
        &self,
        service_id: i32,
        database: &str,
    ) -> Result<Option<service_populate_runs::Model>, ServicePopulateError> {
        Ok(service_populate_runs::Entity::find()
            .filter(service_populate_runs::Column::ServiceId.eq(service_id))
            .filter(service_populate_runs::Column::DatabaseName.eq(database))
            .filter(service_populate_runs::Column::Status.eq(POPULATE_STATUS_RUNNING))
            .one(self.db.as_ref())
            .await?)
    }

    fn map_service_error(&self, service_id: i32, e: ExternalServiceError) -> ServicePopulateError {
        match e {
            ExternalServiceError::ServiceNotFound { .. } => {
                ServicePopulateError::ServiceNotFound { service_id }
            }
            other => ServicePopulateError::ServiceConfig {
                service_id,
                reason: other.to_string(),
            },
        }
    }

    /// Resolve the destination server: a local, standalone PostgreSQL
    /// service with stored credentials and port.
    async fn resolve_target(&self, service_id: i32) -> Result<TargetServer, ServicePopulateError> {
        let service = self
            .external_services
            .get_service(service_id)
            .await
            .map_err(|e| self.map_service_error(service_id, e))?;

        if service.service_type != "postgres" {
            return Err(ServicePopulateError::UnsupportedService {
                service_id,
                reason: format!(
                    "service type is '{}'; only PostgreSQL services can be populated",
                    service.service_type
                ),
            });
        }
        if service.topology != "standalone" {
            return Err(ServicePopulateError::UnsupportedService {
                service_id,
                reason: format!(
                    "topology is '{}'; only standalone PostgreSQL services can be populated",
                    service.topology
                ),
            });
        }
        if let Some(node_id) = service.node_id {
            return Err(ServicePopulateError::UnsupportedService {
                service_id,
                reason: format!(
                    "the service runs on node {}; only services on the control-plane host can be \
                     populated",
                    node_id
                ),
            });
        }
        if !self.external_services.local_workloads_enabled() {
            return Err(ServicePopulateError::LocalWorkloadsDisabled {
                service_id,
                reason: "this process does not run managed-service containers".to_string(),
            });
        }

        let config = self
            .external_services
            .get_service_config(service_id)
            .await
            .map_err(|e| self.map_service_error(service_id, e))?;
        // PostgresInputConfig generates a port and a password when they are
        // missing — right when creating a service, wrong here: they must be
        // the ones the running container was created with.
        for required in ["port", "password"] {
            let present = config
                .parameters
                .get(required)
                .and_then(|v| v.as_str())
                .is_some_and(|v| !v.is_empty());
            if !present {
                return Err(ServicePopulateError::ServiceConfig {
                    service_id,
                    reason: format!("parameter '{}' is not set", required),
                });
            }
        }
        let input: PostgresInputConfig =
            serde_json::from_value(config.parameters).map_err(|e| {
                ServicePopulateError::ServiceConfig {
                    service_id,
                    reason: format!("PostgreSQL parameters do not parse: {}", e),
                }
            })?;
        let pg = PostgresConfig::from(input);
        let admin = self
            .external_services
            .get_service_admin_endpoint(service_id, &pg.host, &pg.port)
            .await;

        Ok(TargetServer {
            service_id,
            host: pg.host,
            port: pg.port,
            admin_host: admin.host,
            admin_port: admin.port,
            username: pg.username,
            password: pg.password,
            major_version: 0,
        })
    }
}

/// The background half of a run.
struct PopulateJob {
    run_id: i32,
    database: String,
    source_url: String,
    preparation: TargetPreparation,
    disconnect_clients: bool,
    client_image: String,
    target: TargetServer,
}

/// Outcome of the background half: the destination size, or a scrubbed
/// error message.
type JobOutcome = Result<Option<i64>, String>;

impl PopulateJob {
    async fn execute(self, docker: &bollard::Docker) -> JobOutcome {
        let secrets = self.secrets();
        let clean = |message: String| {
            strip_row_data(&scrub_secrets(
                &message,
                &secrets.iter().map(String::as_str).collect::<Vec<_>>(),
            ))
        };

        // Re-check right before acting: `start` inspected the destination a
        // moment ago, but a deployment may have created tables since.
        let preparation = if self.preparation == TargetPreparation::UseExisting {
            let state = self
                .target
                .inspect(&self.database)
                .await
                .map_err(|e| clean(e.to_string()))?;
            plan_target_preparation(
                self.target.service_id,
                &self.database,
                state.exists,
                state.table_count,
                false,
            )
            .map_err(|e| clean(e.to_string()))?
        } else {
            self.preparation
        };

        let staging = staging_database_name(&self.database, self.run_id);
        let target = &self.target;
        let source_url = self.source_url.as_str();
        let image = self.client_image.as_str();
        let run_label = (populate_container_label(), self.run_id.to_string());
        copy_into_destination(
            target,
            &self.database,
            &staging,
            preparation,
            self.disconnect_clients,
            |copy_into| async move {
                let destination_url = target.transfer_url(&copy_into);
                run_transfer_container(
                    docker,
                    &TransferContainerSpec {
                        image,
                        command: postgres_populate_command(),
                        source_url,
                        destination_url: &destination_url,
                        credentials: TransferCredentials::PgPassFile,
                        network_mode: "host",
                        name_prefix: "temps-populate",
                        labels: vec![run_label],
                        timeout: DATA_TRANSFER_TIMEOUT,
                    },
                )
                .await
                .map_err(|e| e.to_string())
            },
        )
        .await
        .map_err(clean)?;

        // The copy succeeded; a failed measurement must not turn it into a
        // failure.
        match self.target.database_size(&self.database).await {
            Ok(size) => Ok(Some(size)),
            Err(e) => {
                warn!(
                    run_id = self.run_id,
                    error = %clean(e.to_string()),
                    "Populate finished but the database size could not be measured"
                );
                Ok(None)
            }
        }
    }

    fn secrets(&self) -> Vec<String> {
        let mut secrets = vec![
            self.source_url.clone(),
            self.target.password.clone(),
            self.target.transfer_url(&self.database),
            self.target.admin_url(&self.database),
        ];
        if let Ok(parsed) = url::Url::parse(&self.source_url) {
            if let Some(password) = parsed.password() {
                secrets.push(password.to_string());
            }
        }
        secrets
    }
}

/// Record the outcome. Retried, because a run left `running` blocks the next
/// populate of the same database until the next restart marks it failed.
async fn finalize_run(db: &DatabaseConnection, run_id: i32, outcome: JobOutcome) {
    let (status, error_message, size) = match &outcome {
        Ok(size) => (POPULATE_STATUS_COMPLETED, None, *size),
        Err(message) => (POPULATE_STATUS_FAILED, Some(message.clone()), None),
    };
    let mut last_error = None;
    for attempt in 0..4u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
        }
        let update = service_populate_runs::ActiveModel {
            id: Set(run_id),
            status: Set(status.to_string()),
            error_message: Set(error_message.clone()),
            database_size_bytes: Set(size),
            finished_at: Set(Some(Utc::now())),
            ..Default::default()
        }
        .update(db)
        .await;
        match update {
            Ok(run) => {
                match &outcome {
                    Ok(_) => info!(
                        run_id,
                        service_id = run.service_id,
                        database = %run.database_name,
                        size_bytes = ?size,
                        "Populate run completed"
                    ),
                    Err(message) => warn!(
                        run_id,
                        service_id = run.service_id,
                        database = %run.database_name,
                        error = %message,
                        "Populate run failed"
                    ),
                }
                return;
            }
            Err(e) => {
                warn!(run_id, attempt, error = %e, "Could not record populate outcome, retrying");
                last_error = Some(e);
            }
        }
    }
    error!(
        run_id,
        status,
        error = ?last_error,
        "Gave up recording the outcome of populate run; it stays 'running' until the next \
         restart marks it failed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DatabaseBackend, MockDatabase};

    #[test]
    fn database_name_follows_the_custom_database_identifier_rule() {
        assert!(validate_database_name("my_app_homolog").is_ok());
        assert!(validate_database_name("_app").is_ok());
        for bad in [
            "",
            "App",
            "1app",
            "app-db",
            "app db",
            "app\"; DROP DATABASE x; --",
            &"a".repeat(64),
        ] {
            assert!(
                matches!(
                    validate_database_name(bad),
                    Err(ServicePopulateError::InvalidDatabaseName { .. })
                ),
                "'{}' must be rejected",
                bad
            );
        }
    }

    #[test]
    fn system_databases_are_rejected() {
        for reserved in RESERVED_DATABASES {
            let err = validate_database_name(reserved).expect_err(reserved);
            assert!(err.to_string().contains("system database"), "{}", err);
        }
    }

    #[test]
    fn a_non_empty_destination_is_refused_without_replace() {
        let err = plan_target_preparation(1, "app_production", true, 3, false)
            .expect_err("non-empty database must be refused");
        assert!(matches!(
            err,
            ServicePopulateError::TargetNotEmpty {
                service_id: 1,
                table_count: 3,
                ..
            }
        ));
        assert!(err.to_string().contains("replace=true"), "{}", err);
    }

    #[test]
    fn target_preparation_covers_every_case() {
        assert_eq!(
            plan_target_preparation(1, "db", false, 0, false).unwrap(),
            TargetPreparation::Create
        );
        assert_eq!(
            plan_target_preparation(1, "db", false, 0, true).unwrap(),
            TargetPreparation::Create
        );
        assert_eq!(
            plan_target_preparation(1, "db", true, 0, false).unwrap(),
            TargetPreparation::UseExisting
        );
        assert_eq!(
            plan_target_preparation(1, "db", true, 7, true).unwrap(),
            TargetPreparation::Recreate
        );
    }

    #[test]
    fn source_url_password_is_masked() {
        let masked =
            mask_source_url("postgres://app:s3cr3t%40x@db.example.com:5432/app?sslmode=require");
        assert!(!masked.contains("s3cr3t"), "{}", masked);
        assert!(
            !masked.contains("app:"),
            "user must be masked too: {}",
            masked
        );
        assert!(masked.contains("db.example.com:5432/app"), "{}", masked);
        assert!(masked.contains("sslmode=require"), "{}", masked);
        // Unparseable input is never echoed.
        assert_eq!(mask_source_url("not a url with secret"), "***");
    }

    #[test]
    fn source_url_shape_accepts_sslmode() {
        assert!(validate_source_url_shape("postgres://u:p@db.example.com:5432/app").is_ok());
        assert!(
            validate_source_url_shape("postgresql://u:p@db.example.com/app?sslmode=require")
                .is_ok()
        );
        assert!(validate_source_url_shape(
            "postgres://u:p@db.example.com/app?sslmode=verify-full&connect_timeout=10"
        )
        .is_ok());
    }

    #[test]
    fn source_url_shape_rejects_redirecting_parameters_and_bad_input() {
        for (url, expected) in [
            ("mysql://u:p@db.example.com/app", "scheme"),
            (
                "postgres://u:p@db.example.com:5432/",
                "name the source database",
            ),
            ("postgres://u:p@db.example.com/app?host=127.0.0.1", "'host'"),
            (
                "postgres://u:p@db.example.com/app?hostaddr=10.0.0.1",
                "'hostaddr'",
            ),
            (
                "postgres://u:p@db.example.com/app?dbname=host%3D127.0.0.1",
                "'dbname'",
            ),
            (
                "postgres://u:p@db.example.com/app?sslmode=bogus",
                "unsupported value",
            ),
            (
                "postgres://u:p@db.example.com/host%3D127.0.0.1",
                "may only contain",
            ),
        ] {
            let err = validate_source_url_shape(url).expect_err(url).to_string();
            assert!(err.contains(expected), "{} -> {}", url, err);
        }
    }

    #[test]
    fn source_url_errors_never_echo_the_password() {
        for url in [
            "mysql://u:TOPSECRET@db.example.com/app",
            "postgres://u:TOPSECRET@db.example.com/app?host=127.0.0.1",
            "postgres://u:TOPSECRET@db.example.com/",
        ] {
            let err = validate_source_url_shape(url).expect_err(url).to_string();
            assert!(!err.contains("TOPSECRET"), "{}", err);
        }
    }

    #[tokio::test]
    async fn ssrf_guard_rejects_private_and_loopback_sources() {
        for url in [
            "postgres://u:p@127.0.0.1:5432/app",
            "postgres://u:p@10.0.0.5:5432/app",
            "postgres://u:p@169.254.169.254:5432/app",
            "postgres://u:p@localhost:5432/app",
        ] {
            let err = validate_source_url(url).await.expect_err(url).to_string();
            assert!(err.contains("Invalid source URL"), "{} -> {}", url, err);
            assert!(!err.contains("u:p@"), "{}", err);
        }
    }

    #[test]
    fn scrub_removes_urls_and_passwords_in_both_encodings() {
        let message = "pg_dump: error: connection to postgres://u:p%40ss@h/db failed; \
                       password p@ss rejected; dst postgres://app:x@127.0.0.1:5432/db";
        let scrubbed = scrub_secrets(
            message,
            &[
                "postgres://u:p%40ss@h/db",
                "p%40ss",
                "postgres://app:x@127.0.0.1:5432/db",
            ],
        );
        assert!(!scrubbed.contains("p@ss"), "{}", scrubbed);
        assert!(!scrubbed.contains("p%40ss"), "{}", scrubbed);
        assert!(!scrubbed.contains("app:x"), "{}", scrubbed);
    }

    fn server(host: &str, port: &str, password: &str) -> TargetServer {
        TargetServer {
            service_id: 1,
            host: host.to_string(),
            port: port.to_string(),
            admin_host: host.to_string(),
            admin_port: port.to_string(),
            username: "postgres".to_string(),
            password: password.to_string(),
            major_version: 18,
        }
    }

    /// Control plane in a container: its SQL goes to the container name and
    /// internal port, while the transfer container (host network) keeps the
    /// published port on 127.0.0.1.
    #[test]
    fn admin_url_follows_the_admin_endpoint_and_transfer_keeps_the_published_port() {
        let mut target = server("localhost", "5433", "pw");
        target.admin_host = "postgres-app-db".to_string();
        target.admin_port = "5432".to_string();
        assert_eq!(
            target.admin_url("postgres"),
            "postgres://postgres:pw@postgres-app-db:5432/postgres?sslmode=disable"
        );
        assert_eq!(
            target.transfer_url("app"),
            "postgres://postgres:pw@127.0.0.1:5433/app?sslmode=disable"
        );
    }

    #[test]
    fn transfer_url_uses_ipv4_loopback_and_encodes_credentials() {
        let target = server("localhost", "5433", "a@b:c");
        assert_eq!(
            target.transfer_url("app_production"),
            "postgres://postgres:a%40b%3Ac@127.0.0.1:5433/app_production?sslmode=disable"
        );
        assert_eq!(
            target.admin_url("postgres"),
            "postgres://postgres:a%40b%3Ac@localhost:5433/postgres?sslmode=disable"
        );
    }

    #[test]
    fn staging_name_fits_63_bytes_and_stays_unique() {
        assert_eq!(
            staging_database_name("app_homolog", 7),
            "app_homolog__populate_7"
        );
        let long = "a".repeat(63);
        let staging = staging_database_name(&long, 123_456);
        assert_eq!(staging.len(), 63);
        assert!(staging.ends_with("__populate_123456"));
        assert_ne!(staging, staging_database_name(&long, 123_457));
    }

    #[test]
    fn staging_like_names_are_rejected_as_destinations() {
        let err = validate_database_name("app__populate_3").expect_err("staging name");
        assert!(err.to_string().contains("reserved for staging"), "{}", err);
    }

    #[test]
    fn control_plane_server_is_recognised_in_any_local_spelling() {
        let control_plane =
            DatabaseEndpoint::from_url("postgres://temps:pw@127.0.0.1:5432/temps").expect("url");
        for (host, port) in [("localhost", 5432), ("127.0.0.1", 5432), ("[::1]", 5432)] {
            assert!(
                control_plane.same_server(&DatabaseEndpoint {
                    host: host.to_string(),
                    port
                }),
                "{}:{}",
                host,
                port
            );
        }
        assert!(!control_plane.same_server(&DatabaseEndpoint {
            host: "localhost".to_string(),
            port: 5433
        }));
        assert!(!control_plane.same_server(&DatabaseEndpoint {
            host: "db.example.com".to_string(),
            port: 5432
        }));
        assert_eq!(
            DatabaseEndpoint::from_url("postgresql://u@db.example.com/x"),
            Some(DatabaseEndpoint {
                host: "db.example.com".to_string(),
                port: 5432
            })
        );
        assert_eq!(DatabaseEndpoint::from_url("sqlite://x.db"), None);
    }

    #[test]
    fn source_url_accepts_the_system_ca_store_only() {
        assert!(validate_source_url_shape(
            "postgres://u:p@db.example.com/app?sslmode=verify-full&sslrootcert=system"
        )
        .is_ok());
        let err = validate_source_url_shape(
            "postgres://u:p@db.example.com/app?sslmode=verify-full&sslrootcert=/etc/passwd",
        )
        .expect_err("file path")
        .to_string();
        assert!(err.contains("unsupported value"), "{}", err);
    }

    #[test]
    fn row_data_is_stripped_from_error_output() {
        let output = "transfer exited with status 3: psql:<stdin>:40: ERROR:  invalid input \
                      syntax for type integer\nCONTEXT:  COPY customers, line 1, column id: \
                      \"secret-row-value\"\nDETAIL:  Key (email)=(someone@example.com) exists.";
        let stripped = strip_row_data(output);
        assert!(stripped.contains("invalid input syntax"), "{}", stripped);
        assert!(!stripped.contains("secret-row-value"), "{}", stripped);
        assert!(!stripped.contains("someone@example.com"), "{}", stripped);
    }

    fn sample_run(id: i32, service_id: i32) -> service_populate_runs::Model {
        let now = Utc::now();
        service_populate_runs::Model {
            id,
            service_id,
            database_name: "app_homolog".to_string(),
            source_url_masked: "postgres://***:***@db.example.com:5432/app".to_string(),
            replace_existing: false,
            disconnect_clients: false,
            status: POPULATE_STATUS_COMPLETED.to_string(),
            client_image: "postgres:18-alpine".to_string(),
            error_message: None,
            database_size_bytes: Some(8_000_000),
            started_at: now,
            finished_at: Some(now),
            created_by: Some(1),
            created_at: now,
            updated_at: now,
        }
    }

    fn service_with_mock(db: DatabaseConnection) -> ServicePopulateService {
        service_with_shared_mock(Arc::new(db))
    }

    fn service_with_shared_mock(db: Arc<DatabaseConnection>) -> ServicePopulateService {
        let encryption = Arc::new(
            temps_core::EncryptionService::new(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            )
            .expect("test encryption key"),
        );
        let docker = Arc::new(DockerHandle::disabled("test", "no docker in unit tests"));
        let manager = Arc::new(ExternalServiceManager::new_with_handle(
            db.clone(),
            encryption,
            docker.clone(),
            true,
            Arc::new(temps_dns::DnsRegistry::new(db.clone())),
        ));
        ServicePopulateService::new(db, manager, docker, None)
    }

    #[tokio::test]
    async fn get_run_returns_the_run_of_the_service() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![sample_run(7, 3)]])
            .into_connection();
        let run = service_with_mock(db).get_run(3, 7).await.expect("run");
        assert_eq!(run.id, 7);
        assert_eq!(run.status, POPULATE_STATUS_COMPLETED);
    }

    #[tokio::test]
    async fn get_run_of_another_service_is_not_found() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([Vec::<service_populate_runs::Model>::new()])
            .into_connection();
        let err = service_with_mock(db).get_run(3, 7).await.expect_err("404");
        assert!(matches!(
            err,
            ServicePopulateError::RunNotFound {
                service_id: 3,
                run_id: 7
            }
        ));
    }

    #[tokio::test]
    async fn start_rejects_a_bad_database_name_before_touching_anything() {
        // No query results queued: any database access would panic the mock.
        let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
        let err = service_with_mock(db)
            .start(StartPopulateRequest {
                service_id: 1,
                database: "postgres".to_string(),
                source_url: "postgres://u:p@db.example.com/app".to_string(),
                replace: false,
                disconnect_clients: false,
                created_by: Some(1),
            })
            .await
            .expect_err("reserved name");
        assert!(matches!(
            err,
            ServicePopulateError::InvalidDatabaseName { .. }
        ));
    }

    #[tokio::test]
    async fn recovery_only_fails_runs_started_before_the_process() {
        let boot = Utc::now();
        let mut old = sample_run(4, 1);
        old.status = POPULATE_STATUS_RUNNING.to_string();
        let mut failed = old.clone();
        failed.status = POPULATE_STATUS_FAILED.to_string();
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![old]])
            .append_query_results([vec![failed]])
            .into_connection();
        let db = Arc::new(db);
        let service = service_with_shared_mock(db.clone());
        let (containers, runs) = service
            .recover_interrupted_runs(boot)
            .await
            .expect("recover");
        assert_eq!((containers, runs), (0, 1));

        drop(service);
        let log = Arc::try_unwrap(db)
            .unwrap_or_else(|_| panic!("mock connection still shared"))
            .into_transaction_log();
        let select = &log[0].statements()[0];
        assert!(select.sql.contains("\"status\" = $1"), "{}", select.sql);
        assert!(select.sql.contains("\"started_at\" < $2"), "{}", select.sql);
        let update = format!("{:?}", log[1].statements()[0].values);
        assert!(update.contains("__populate_4"), "{}", update);
    }

    // ---- End to end, against real servers (skips without Docker) -------

    use testcontainers::{
        core::{ContainerPort, WaitFor},
        runners::AsyncRunner,
        ContainerAsync, GenericImage, ImageExt,
    };

    const READY: &str = "database system is ready to accept connections";

    struct Servers {
        docker: bollard::Docker,
        network: String,
        source_name: String,
        source: ContainerAsync<GenericImage>,
        target: ContainerAsync<GenericImage>,
        /// Destination as the test process reaches it (published port).
        server: TargetServer,
        /// Destination as the transfer container reaches it (Docker network).
        in_network: TargetServer,
        /// Source as the test process reaches it (published port).
        source_admin_url: String,
    }

    impl Servers {
        async fn start() -> Option<Self> {
            let docker = bollard::Docker::connect_with_local_defaults().ok()?;
            if docker.ping().await.is_err() {
                return None;
            }
            let suffix = uuid::Uuid::new_v4().to_string()[..8].to_string();
            let network = format!("temps-populate-test-{}", suffix);
            let source_name = format!("temps-populate-src-{}", suffix);
            let target_name = format!("temps-populate-dst-{}", suffix);

            let source = GenericImage::new("postgres", "16-alpine")
                .with_exposed_port(ContainerPort::Tcp(5432))
                .with_wait_for(WaitFor::message_on_stderr(READY))
                .with_env_var("POSTGRES_PASSWORD", "source-pass")
                .with_env_var("POSTGRES_DB", "app")
                .with_network(&network)
                .with_container_name(&source_name)
                .start()
                .await
                .expect("start PG16 source");
            let target = GenericImage::new("postgres", "18-alpine")
                .with_exposed_port(ContainerPort::Tcp(5432))
                .with_wait_for(WaitFor::message_on_stderr(READY))
                .with_env_var("POSTGRES_USER", "svc_owner")
                .with_env_var("POSTGRES_PASSWORD", "dst p@ss:word")
                .with_network(&network)
                .with_container_name(&target_name)
                .start()
                .await
                .expect("start PG18 destination");

            let mut server = server(
                &target.get_host().await.expect("host").to_string(),
                &target
                    .get_host_port_ipv4(5432)
                    .await
                    .expect("port")
                    .to_string(),
                "dst p@ss:word",
            );
            server.username = "svc_owner".to_string();
            let mut in_network = server.clone();
            in_network.host = target_name.clone();
            in_network.port = "5432".to_string();
            in_network.admin_host = target_name;
            in_network.admin_port = "5432".to_string();
            let source_admin_url = format!(
                "postgres://postgres:source-pass@{}:{}/app?sslmode=disable",
                source.get_host().await.expect("host"),
                source.get_host_port_ipv4(5432).await.expect("port")
            );

            // Both servers answer over TCP (the first "ready" line is the
            // init server, which listens on no TCP socket).
            for _ in 0..60 {
                let src_ok = sqlx::PgConnection::connect(&source_admin_url).await.is_ok();
                if src_ok && server.inspect("postgres").await.is_ok() {
                    return Some(Self {
                        docker,
                        network,
                        source_name,
                        source,
                        target,
                        server,
                        in_network,
                        source_admin_url,
                    });
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            panic!("test servers never became reachable");
        }

        fn source_url(&self, database: &str, password: &str) -> String {
            format!(
                "postgres://postgres:{}@{}:5432/{}?sslmode=disable",
                password, self.source_name, database
            )
        }

        async fn source_sql(&self, sql: &str) {
            let mut conn = sqlx::PgConnection::connect(&self.source_admin_url)
                .await
                .expect("connect source");
            sqlx::raw_sql(sql).execute(&mut conn).await.expect(sql);
            let _ = conn.close().await;
        }

        /// The transfer exactly as a run does it, on the test network.
        async fn transfer(&self, source_url: &str, into: &str) -> Result<(), String> {
            run_transfer_container(
                &self.docker,
                &TransferContainerSpec {
                    image: "postgres:18-alpine",
                    command: postgres_populate_command(),
                    source_url,
                    destination_url: &self.in_network.admin_url(into),
                    credentials: TransferCredentials::PgPassFile,
                    network_mode: &self.network,
                    name_prefix: "temps-populate-test",
                    labels: vec![(populate_container_label(), "0".to_string())],
                    timeout: Duration::from_secs(300),
                },
            )
            .await
            .map_err(|e| e.to_string())
        }

        async fn populate(
            &self,
            database: &str,
            run_id: i32,
            preparation: TargetPreparation,
            disconnect_clients: bool,
            source_url: &str,
        ) -> Result<(), String> {
            copy_into_destination(
                &self.server,
                database,
                &staging_database_name(database, run_id),
                preparation,
                disconnect_clients,
                |into| async move { self.transfer(source_url, &into).await },
            )
            .await
        }

        async fn scalar_i64(&self, database: &str, sql: &str) -> i64 {
            let mut conn = self.server.connect(database).await.expect("connect");
            let value: i64 = sqlx::query_scalar(sql)
                .fetch_one(&mut conn)
                .await
                .expect(sql);
            let _ = conn.close().await;
            value
        }

        async fn staging_databases(&self) -> i64 {
            self.scalar_i64(
                "postgres",
                "SELECT count(*) FROM pg_database WHERE datname LIKE '%\\_\\_populate\\_%'",
            )
            .await
        }

        async fn stop(self) {
            let _ = self.target.rm().await;
            let _ = self.source.rm().await;
        }
    }

    /// A PG16 source into a PG18 destination owned by a non-default user,
    /// through the staging database: create, refuse a non-empty destination,
    /// replace without duplicating, refuse a replace while clients are
    /// connected (unless told to disconnect them), and keep the destination
    /// intact when the copy fails.
    #[tokio::test]
    async fn populate_copies_a_pg16_source_into_a_pg18_service_database() {
        let Some(servers) = Servers::start().await else {
            println!("Docker not available, skipping");
            return;
        };
        servers
            .source_sql(
                "CREATE TABLE customers (id serial PRIMARY KEY, name text NOT NULL); \
                 INSERT INTO customers (name) VALUES ('a'), ('b'), ('c');",
            )
            .await;
        let source = servers.source_url("app", "source-pass");
        let server = &servers.server;
        let rows = "SELECT count(*) FROM customers";

        let state = server.inspect("app_homolog").await.expect("inspect");
        assert_eq!(state.major_version, 18);
        assert_eq!(
            postgres_client_image(state.major_version),
            "postgres:18-alpine"
        );
        let preparation =
            plan_target_preparation(1, "app_homolog", state.exists, state.table_count, false)
                .expect("plan");
        assert_eq!(preparation, TargetPreparation::Create);

        // Create: through the staging database, renamed on success.
        servers
            .populate("app_homolog", 1, preparation, false, &source)
            .await
            .expect("populate PG16 -> PG18");
        assert_eq!(servers.scalar_i64("app_homolog", rows).await, 3);
        assert_eq!(servers.staging_databases().await, 0);
        let mut admin = server.connect("postgres").await.expect("admin");
        let owner: String = sqlx::query_scalar(
            "SELECT pg_get_userbyid(datdba)::text FROM pg_database WHERE datname = $1",
        )
        .bind("app_homolog")
        .fetch_one(&mut admin)
        .await
        .expect("owner");
        let _ = admin.close().await;
        assert_eq!(owner, "svc_owner");
        assert!(server.database_size("app_homolog").await.expect("size") > 0);

        // Not empty any more: refused without replace.
        let state = server.inspect("app_homolog").await.expect("inspect");
        assert!(matches!(
            plan_target_preparation(1, "app_homolog", state.exists, state.table_count, false),
            Err(ServicePopulateError::TargetNotEmpty { table_count: 1, .. })
        ));

        // replace while a client is connected: refused at the swap, the
        // destination keeps its data and the staging copy is discarded.
        let mut client = server.connect("app_homolog").await.expect("client");
        assert_eq!(
            server
                .active_sessions("app_homolog")
                .await
                .expect("sessions"),
            1
        );
        let err = servers
            .populate(
                "app_homolog",
                2,
                TargetPreparation::Recreate,
                false,
                &source,
            )
            .await
            .expect_err("open session refuses replace");
        assert!(err.contains("open session"), "{}", err);
        assert_eq!(servers.scalar_i64("app_homolog", rows).await, 3);
        assert_eq!(servers.staging_databases().await, 0);

        // ... and goes through with disconnect_clients, without duplicating.
        servers
            .populate("app_homolog", 3, TargetPreparation::Recreate, true, &source)
            .await
            .expect("replace disconnecting clients");
        assert!(
            sqlx::query("SELECT 1").execute(&mut client).await.is_err(),
            "the old session must have been terminated"
        );
        assert_eq!(servers.scalar_i64("app_homolog", rows).await, 3);
        assert_eq!(servers.staging_databases().await, 0);

        // A replace whose copy fails (wrong source password: pg_dump cannot
        // authenticate) never touches the destination.
        let err = servers
            .populate(
                "app_homolog",
                4,
                TargetPreparation::Recreate,
                true,
                &servers.source_url("app", "wrong-pass"),
            )
            .await
            .expect_err("a failing pg_dump must fail the run");
        assert!(err.contains("exited with status"), "{}", err);
        assert!(!err.contains("wrong-pass"), "{}", err);
        assert_eq!(servers.scalar_i64("app_homolog", rows).await, 3);
        assert_eq!(servers.staging_databases().await, 0);

        // A failed Create leaves no database at all under the final name.
        let err = servers
            .populate(
                "app_new",
                5,
                TargetPreparation::Create,
                false,
                &servers.source_url("app", "wrong-pass"),
            )
            .await
            .expect_err("create with failing copy");
        assert!(err.contains("exited with status"), "{}", err);
        assert!(!server.database_exists("app_new").await.expect("exists"));
        assert_eq!(servers.staging_databases().await, 0);

        servers.stop().await;
    }

    /// Regression: the source connection dies in the middle of a large COPY.
    /// `psql --single-transaction` used to COMMIT at the clean EOF that
    /// followed, leaving a half-filled table; the COMMIT is now only sent
    /// after pg_dump exited 0, so the destination keeps no table at all.
    #[tokio::test]
    async fn a_copy_cut_mid_way_leaves_the_destination_without_tables() {
        let Some(servers) = Servers::start().await else {
            println!("Docker not available, skipping");
            return;
        };
        servers
            .source_sql(
                "CREATE TABLE bulk (id bigint PRIMARY KEY, pad text NOT NULL); \
                 INSERT INTO bulk SELECT g, repeat(md5(g::text), 8) \
                 FROM generate_series(1, 1500000) g;",
            )
            .await;
        servers
            .server
            .create_database("app_cut")
            .await
            .expect("create empty destination");

        let source = servers.source_url("app", "source-pass");
        let copy = servers.populate("app_cut", 6, TargetPreparation::UseExisting, false, &source);
        let killer = async {
            let mut conn = sqlx::PgConnection::connect(&servers.source_admin_url)
                .await
                .expect("connect source");
            for _ in 0..600 {
                let killed: Option<bool> = sqlx::query_scalar(
                    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
                     WHERE query LIKE 'COPY public.bulk%' AND pid <> pg_backend_pid() LIMIT 1",
                )
                .fetch_optional(&mut conn)
                .await
                .expect("terminate");
                if killed == Some(true) {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            false
        };
        let (result, killed) = tokio::join!(copy, killer);
        assert!(killed, "the COPY was never seen on the source");
        let err = result.expect_err("a cut copy must fail");
        assert!(err.contains("exited with status"), "{}", err);

        let tables = servers
            .scalar_i64(
                "app_cut",
                "SELECT count(*) FROM pg_catalog.pg_class c \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p')",
            )
            .await;
        assert_eq!(tables, 0, "a half-copied table was committed");

        servers.stop().await;
    }
}
