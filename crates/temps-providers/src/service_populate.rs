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
//!    and topology, whether the destination is empty — inserts a
//!    `service_populate_runs` row in `running` and spawns the copy. The HTTP
//!    request returns immediately with the run.
//! 2. The background task (re)creates the destination database the same way
//!    the provider's provisioning does (`CREATE DATABASE` as the service
//!    user, so the user owns it), runs the shared transfer container
//!    (`pg_dump | psql`, see [`crate::data_transfer`]) bounded by
//!    [`DATA_TRANSFER_TIMEOUT`], measures the result and finalizes the row.
//!
//! The source URL is never persisted, logged or returned: the row keeps a
//! copy with user and password masked, and every error message that could
//! have picked the URL or a password up is scrubbed before it is stored.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
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
    run_transfer_container, TransferContainerSpec, DATA_TRANSFER_TIMEOUT,
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
const ALLOWED_SOURCE_QUERY_PARAMS: [&str; 3] = ["sslmode", "connect_timeout", "application_name"];

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

#[derive(Debug, Error)]
pub enum ServicePopulateError {
    #[error("Service {service_id} not found")]
    ServiceNotFound { service_id: i32 },

    #[error("Service {service_id} cannot be populated: {reason}")]
    UnsupportedService { service_id: i32, reason: String },

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
    pub created_by: Option<i32>,
}

/// What has to happen to the destination database before the copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPreparation {
    /// The database does not exist yet: create it.
    Create,
    /// The database exists and has no tables in `public`: copy into it.
    UseExisting,
    /// The database exists and `replace` was requested: drop and recreate.
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

/// Validate the destination database name with the same identifier rule the
/// project-link provisioning uses for custom names (`[a-z_][a-z0-9_]{0,62}`),
/// and refuse the server's own databases.
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
    Ok(())
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

/// Everything the background copy needs about the destination server.
#[derive(Clone)]
struct TargetServer {
    service_id: i32,
    /// Host the control plane connects to (the provider's `host` parameter).
    host: String,
    port: String,
    username: String,
    password: String,
}

impl TargetServer {
    fn url_for(&self, host: &str, database: &str) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode=disable",
            percent_encode_userinfo(&self.username),
            percent_encode_userinfo(&self.password),
            host,
            self.port,
            database
        )
    }

    /// Connection URL for control-plane SQL, the way the provider's own
    /// `create_database` connects.
    fn admin_url(&self, database: &str) -> String {
        self.url_for(&self.host, database)
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
        self.url_for(host, database)
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

    /// Drop and/or create `database`. `CREATE DATABASE` runs as the service
    /// user — exactly what the provider's provisioning does — so the user
    /// owns it and a later deployment finds it and skips creating it.
    async fn prepare(
        &self,
        database: &str,
        preparation: TargetPreparation,
        major_version: u32,
    ) -> Result<(), ServicePopulateError> {
        // `database` passed validate_database_name ([a-z_][a-z0-9_]*), so the
        // quoted identifier cannot be broken out of.
        let mut admin = self.connect("postgres").await?;
        if preparation == TargetPreparation::Recreate {
            // WITH (FORCE) (PG13+) terminates the sessions still attached —
            // replacing a database the application is connected to is the
            // point of `replace`.
            let drop = if major_version >= 13 {
                format!("DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)", database)
            } else {
                format!("DROP DATABASE IF EXISTS \"{}\"", database)
            };
            sqlx::query(&drop)
                .execute(&mut admin)
                .await
                .map_err(|e| self.sql_error(&format!("dropping database '{}'", database), e))?;
        }
        if preparation != TargetPreparation::UseExisting {
            sqlx::query(&format!("CREATE DATABASE \"{}\"", database))
                .execute(&mut admin)
                .await
                .map_err(|e| self.sql_error(&format!("creating database '{}'", database), e))?;
        }
        let _ = admin.close().await;
        Ok(())
    }

    async fn database_size(&self, database: &str) -> Result<i64, ServicePopulateError> {
        let mut admin = self.connect("postgres").await?;
        let size: i64 = sqlx::query_scalar("SELECT pg_database_size($1)")
            .bind(database)
            .fetch_one(&mut admin)
            .await
            .map_err(|e| self.sql_error("measuring database size", e))?;
        let _ = admin.close().await;
        Ok(size)
    }
}

#[derive(Debug, Clone, Copy)]
struct TargetState {
    major_version: u32,
    exists: bool,
    table_count: i64,
}

/// Starts populate runs and answers questions about them.
pub struct ServicePopulateService {
    db: Arc<DatabaseConnection>,
    external_services: Arc<ExternalServiceManager>,
    docker: Arc<DockerHandle>,
}

impl ServicePopulateService {
    pub fn new(
        db: Arc<DatabaseConnection>,
        external_services: Arc<ExternalServiceManager>,
        docker: Arc<DockerHandle>,
    ) -> Self {
        Self {
            db,
            external_services,
            docker,
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

        let target = self.resolve_target(service_id).await?;
        // The copy always runs a container: fail before recording anything
        // when this process cannot run one.
        let docker = self.docker.require()?;

        let state = target.inspect(&request.database).await?;
        let preparation = plan_target_preparation(
            service_id,
            &request.database,
            state.exists,
            state.table_count,
            request.replace,
        )?;
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
            client_image = %run.client_image,
            "Starting populate of a managed PostgreSQL database"
        );

        let job = PopulateJob {
            run_id: run.id,
            database: request.database,
            source_url: request.source_url,
            preparation,
            major_version: state.major_version,
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

    /// Mark runs left `running` by a previous process as failed. The copy
    /// lived in that process's task and its container was removed with it
    /// or will never be awaited, so nothing will ever finish them.
    pub async fn fail_interrupted_runs(&self) -> Result<u64, ServicePopulateError> {
        let result = service_populate_runs::Entity::update_many()
            .col_expr(
                service_populate_runs::Column::Status,
                sea_orm::sea_query::Expr::value(POPULATE_STATUS_FAILED),
            )
            .col_expr(
                service_populate_runs::Column::ErrorMessage,
                sea_orm::sea_query::Expr::value(
                    "interrupted: the Temps server restarted while the copy was running; \
                     run populate again (with replace=true if the database is not empty)",
                ),
            )
            .col_expr(
                service_populate_runs::Column::FinishedAt,
                sea_orm::sea_query::Expr::value(Utc::now()),
            )
            .col_expr(
                service_populate_runs::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(Utc::now()),
            )
            .filter(service_populate_runs::Column::Status.eq(POPULATE_STATUS_RUNNING))
            .exec(self.db.as_ref())
            .await?;
        Ok(result.rows_affected)
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

        Ok(TargetServer {
            service_id,
            host: pg.host,
            port: pg.port,
            username: pg.username,
            password: pg.password,
        })
    }
}

/// The background half of a run.
struct PopulateJob {
    run_id: i32,
    database: String,
    source_url: String,
    preparation: TargetPreparation,
    major_version: u32,
    client_image: String,
    target: TargetServer,
}

/// Outcome of the background half: the destination size, or a scrubbed
/// error message.
type JobOutcome = Result<Option<i64>, String>;

impl PopulateJob {
    async fn execute(self, docker: &bollard::Docker) -> JobOutcome {
        let secrets = self.secrets();
        let scrub = |message: String| {
            scrub_secrets(
                &message,
                &secrets.iter().map(String::as_str).collect::<Vec<_>>(),
            )
        };

        // Re-check right before acting: `start` inspected the destination a
        // moment ago, but a deployment may have created tables since.
        let preparation = if self.preparation == TargetPreparation::UseExisting {
            let state = self
                .target
                .inspect(&self.database)
                .await
                .map_err(|e| scrub(e.to_string()))?;
            plan_target_preparation(
                self.target.service_id,
                &self.database,
                state.exists,
                state.table_count,
                false,
            )
            .map_err(|e| scrub(e.to_string()))?
        } else {
            self.preparation
        };

        self.target
            .prepare(&self.database, preparation, self.major_version)
            .await
            .map_err(|e| scrub(e.to_string()))?;

        let destination_url = self.target.transfer_url(&self.database);
        run_transfer_container(
            docker,
            &TransferContainerSpec {
                image: &self.client_image,
                command: postgres_populate_command(),
                source_url: &self.source_url,
                destination_url: &destination_url,
                network_mode: "host",
                name_prefix: "temps-populate",
                timeout: DATA_TRANSFER_TIMEOUT,
            },
        )
        .await
        .map_err(|e| scrub(e.to_string()))?;

        // The copy succeeded; a failed measurement must not turn it into a
        // failure.
        match self.target.database_size(&self.database).await {
            Ok(size) => Ok(Some(size)),
            Err(e) => {
                warn!(
                    run_id = self.run_id,
                    error = %scrub(e.to_string()),
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

async fn finalize_run(db: &DatabaseConnection, run_id: i32, outcome: JobOutcome) {
    let now = Utc::now();
    let (status, error_message, size) = match &outcome {
        Ok(size) => (POPULATE_STATUS_COMPLETED, None, *size),
        Err(message) => (POPULATE_STATUS_FAILED, Some(message.clone()), None),
    };
    let update = service_populate_runs::ActiveModel {
        id: Set(run_id),
        status: Set(status.to_string()),
        error_message: Set(error_message.clone()),
        database_size_bytes: Set(size),
        finished_at: Set(Some(now)),
        ..Default::default()
    }
    .update(db)
    .await;

    match (&outcome, update) {
        (_, Err(e)) => error!(
            run_id,
            status,
            error = %e,
            "Failed to record the outcome of populate run"
        ),
        (Ok(_), Ok(run)) => info!(
            run_id,
            service_id = run.service_id,
            database = %run.database_name,
            size_bytes = ?size,
            "Populate run completed"
        ),
        (Err(message), Ok(run)) => warn!(
            run_id,
            service_id = run.service_id,
            database = %run.database_name,
            error = %message,
            "Populate run failed"
        ),
    }
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

    #[test]
    fn transfer_url_uses_ipv4_loopback_and_encodes_credentials() {
        let target = TargetServer {
            service_id: 1,
            host: "localhost".to_string(),
            port: "5433".to_string(),
            username: "postgres".to_string(),
            password: "a@b:c".to_string(),
        };
        assert_eq!(
            target.transfer_url("app_production"),
            "postgres://postgres:a%40b%3Ac@127.0.0.1:5433/app_production?sslmode=disable"
        );
        assert_eq!(
            target.admin_url("postgres"),
            "postgres://postgres:a%40b%3Ac@localhost:5433/postgres?sslmode=disable"
        );
    }

    fn sample_run(id: i32, service_id: i32) -> service_populate_runs::Model {
        let now = Utc::now();
        service_populate_runs::Model {
            id,
            service_id,
            database_name: "app_homolog".to_string(),
            source_url_masked: "postgres://***:***@db.example.com:5432/app".to_string(),
            replace_existing: false,
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
        let db = Arc::new(db);
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
        ServicePopulateService::new(db, manager, docker)
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

    /// End to end against real servers: a PG16 source with one table, a PG18
    /// destination server running as a non-default user. Exercises the same
    /// pieces a run does — destination inspection, preparation, the shared
    /// transfer container with the populate command and the client image
    /// chosen from the destination version — then checks the rows, the
    /// database owner, the refusal of a non-empty destination, `replace`, and
    /// that a failing `pg_dump` fails the run.
    ///
    /// The transfer container joins a user-defined Docker network instead of
    /// the host network production uses, so the test also runs on Docker
    /// Desktop. Skips when Docker is unavailable.
    #[tokio::test]
    async fn populate_copies_a_pg16_source_into_a_pg18_service_database() {
        use testcontainers::{
            core::{ContainerPort, WaitFor},
            runners::AsyncRunner,
            GenericImage, ImageExt,
        };

        let docker = match bollard::Docker::connect_with_local_defaults() {
            Ok(docker) => docker,
            Err(e) => {
                println!("Docker not available, skipping: {}", e);
                return;
            }
        };
        if docker.ping().await.is_err() {
            println!("Docker not available, skipping");
            return;
        }

        let suffix = &uuid::Uuid::new_v4().to_string()[..8];
        let network = format!("temps-populate-test-{}", suffix);
        let source_name = format!("temps-populate-src-{}", suffix);
        let target_name = format!("temps-populate-dst-{}", suffix);
        let ready = "database system is ready to accept connections";

        let source = GenericImage::new("postgres", "16-alpine")
            .with_wait_for(WaitFor::message_on_stderr(ready))
            .with_env_var("POSTGRES_PASSWORD", "source-pass")
            .with_env_var("POSTGRES_DB", "app")
            .with_network(&network)
            .with_container_name(&source_name)
            .start()
            .await
            .expect("start PG16 source");
        let target = GenericImage::new("postgres", "18-alpine")
            .with_exposed_port(ContainerPort::Tcp(5432))
            .with_wait_for(WaitFor::message_on_stderr(ready))
            .with_env_var("POSTGRES_USER", "svc_owner")
            .with_env_var("POSTGRES_PASSWORD", "dst p@ss:word")
            .with_network(&network)
            .with_container_name(&target_name)
            .start()
            .await
            .expect("start PG18 destination");

        let server = TargetServer {
            service_id: 1,
            host: target.get_host().await.expect("host").to_string(),
            port: target
                .get_host_port_ipv4(5432)
                .await
                .expect("mapped port")
                .to_string(),
            username: "svc_owner".to_string(),
            password: "dst p@ss:word".to_string(),
        };
        let source_url = format!(
            "postgres://postgres:source-pass@{}:5432/app?sslmode=disable",
            source_name
        );
        let destination_url = format!(
            "postgres://svc_owner:{}@{}:5432/app_homolog?sslmode=disable",
            percent_encode_userinfo("dst p@ss:word"),
            target_name
        );
        let transfer = |image: String, command: &'static str, source: String| {
            let docker = docker.clone();
            let network = network.clone();
            let destination_url = destination_url.clone();
            async move {
                run_transfer_container(
                    &docker,
                    &TransferContainerSpec {
                        image: &image,
                        command,
                        source_url: &source,
                        destination_url: &destination_url,
                        network_mode: &network,
                        name_prefix: "temps-populate-test",
                        timeout: Duration::from_secs(300),
                    },
                )
                .await
            }
        };

        // Seed the source. The first "ready" line is the init server, which
        // listens on no TCP socket: wait for the real one.
        transfer(
            "postgres:16-alpine".to_string(),
            "for i in $(seq 1 60); do pg_isready -d \"$SRC\" >/dev/null 2>&1 && break; sleep 1; done; \
             psql -v ON_ERROR_STOP=1 \"$SRC\" -c \"CREATE TABLE customers (id serial PRIMARY KEY, name text NOT NULL); \
             INSERT INTO customers (name) VALUES ('a'), ('b'), ('c');\"",
            source_url.clone(),
        )
        .await
        .expect("seed the source");

        // Destination server reachable from the test process.
        let mut state = None;
        for _ in 0..60 {
            if let Ok(s) = server.inspect("app_homolog").await {
                state = Some(s);
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let state = state.expect("destination server never became reachable");
        assert_eq!(state.major_version, 18);
        assert!(!state.exists);
        let image = postgres_client_image(state.major_version);
        assert_eq!(image, "postgres:18-alpine");

        // First populate: the database is created and filled.
        let preparation =
            plan_target_preparation(1, "app_homolog", state.exists, state.table_count, false)
                .expect("missing database is created");
        assert_eq!(preparation, TargetPreparation::Create);
        server
            .prepare("app_homolog", preparation, state.major_version)
            .await
            .expect("create destination");
        transfer(
            image.clone(),
            postgres_populate_command(),
            source_url.clone(),
        )
        .await
        .expect("populate PG16 -> PG18 with the PG18 client");

        let count_rows = || async {
            let mut conn = server.connect("app_homolog").await.expect("connect");
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM customers")
                .fetch_one(&mut conn)
                .await
                .expect("count rows");
            count
        };
        assert_eq!(count_rows().await, 3);

        // Owned by the service user, as the provider's provisioning creates it.
        let mut admin = server.connect("postgres").await.expect("admin");
        let owner: String = sqlx::query_scalar(
            "SELECT pg_get_userbyid(datdba)::text FROM pg_database WHERE datname = $1",
        )
        .bind("app_homolog")
        .fetch_one(&mut admin)
        .await
        .expect("owner");
        assert_eq!(owner, "svc_owner");
        assert!(server.database_size("app_homolog").await.expect("size") > 0);

        // Not empty any more: refused without replace.
        let state = server.inspect("app_homolog").await.expect("inspect");
        assert_eq!(state.table_count, 1);
        assert!(matches!(
            plan_target_preparation(1, "app_homolog", state.exists, state.table_count, false),
            Err(ServicePopulateError::TargetNotEmpty { .. })
        ));

        // replace: dropped and recreated, so the rows are not duplicated.
        let preparation =
            plan_target_preparation(1, "app_homolog", state.exists, state.table_count, true)
                .expect("replace");
        assert_eq!(preparation, TargetPreparation::Recreate);
        server
            .prepare("app_homolog", preparation, state.major_version)
            .await
            .expect("recreate destination");
        transfer(
            image.clone(),
            postgres_populate_command(),
            source_url.clone(),
        )
        .await
        .expect("populate again");
        assert_eq!(count_rows().await, 3);

        // A pg_dump that cannot authenticate fails the transfer (pipefail),
        // instead of feeding psql an empty script and "succeeding".
        let error = transfer(
            image,
            postgres_populate_command(),
            source_url.replace("source-pass", "wrong-pass"),
        )
        .await
        .expect_err("a failing pg_dump must fail the run")
        .to_string();
        assert!(error.contains("exited with status"), "{}", error);
        assert!(!error.contains("wrong-pass"), "{}", error);

        let _ = admin.close().await;
        let _ = target.rm().await;
        let _ = source.rm().await;
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
                created_by: Some(1),
            })
            .await
            .expect_err("reserved name");
        assert!(matches!(
            err,
            ServicePopulateError::InvalidDatabaseName { .. }
        ));
    }
}
