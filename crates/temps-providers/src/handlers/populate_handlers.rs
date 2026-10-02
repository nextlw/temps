// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! HTTP handlers for populating a database of an existing PostgreSQL service
//! from an external PostgreSQL.
//!
//! # Routes
//!
//! ```text
//! POST /external-services/{id}/populate                  start a copy (202 + run)
//! GET  /external-services/{id}/populate-runs             list runs, newest first
//! GET  /external-services/{id}/populate-runs/{run_id}    one run
//! ```
//!
//! Every route is restricted to an instance administrator: a populate can
//! drop a database, and the runs name the databases of every project the
//! service hosts. The source URL travels only in the request body; responses
//! carry it with user and password masked.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use temps_auth::{permission_guard, AuthContext, RequireAuth};
use temps_core::problemdetails::{self, Problem, ProblemDetails};
use temps_core::{AuditContext, AuditLogger, AuditOperation, RequestMetadata};
use temps_entities::service_populate_runs;
use tracing::error;
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::service_populate::{ServicePopulateError, ServicePopulateService, StartPopulateRequest};

/// Dependencies of the populate routes.
pub struct PopulateAppState {
    pub populate_service: Arc<ServicePopulateService>,
    pub audit_service: Arc<dyn AuditLogger>,
}

impl From<ServicePopulateError> for Problem {
    fn from(error: ServicePopulateError) -> Self {
        match error {
            ServicePopulateError::ServiceNotFound { .. } => {
                problemdetails::new(StatusCode::NOT_FOUND)
                    .with_title("Service Not Found")
                    .with_detail(error.to_string())
            }
            ServicePopulateError::RunNotFound { .. } => problemdetails::new(StatusCode::NOT_FOUND)
                .with_title("Populate Run Not Found")
                .with_detail(error.to_string()),
            ServicePopulateError::UnsupportedService { .. } => {
                problemdetails::new(StatusCode::UNPROCESSABLE_ENTITY)
                    .with_title("Service Cannot Be Populated")
                    .with_detail(error.to_string())
            }
            ServicePopulateError::InvalidDatabaseName { .. } => {
                problemdetails::new(StatusCode::BAD_REQUEST)
                    .with_title("Invalid Database Name")
                    .with_detail(error.to_string())
            }
            ServicePopulateError::InvalidSourceUrl { .. } => {
                problemdetails::new(StatusCode::BAD_REQUEST)
                    .with_title("Invalid Source URL")
                    .with_detail(error.to_string())
            }
            ServicePopulateError::TargetNotEmpty { .. } => {
                problemdetails::new(StatusCode::CONFLICT)
                    .with_title("Database Not Empty")
                    .with_detail(error.to_string())
            }
            ServicePopulateError::AlreadyRunning { .. } => {
                problemdetails::new(StatusCode::CONFLICT)
                    .with_title("Populate Already Running")
                    .with_detail(error.to_string())
            }
            ServicePopulateError::LocalWorkloadsDisabled { .. } => {
                temps_core::worker_node_required_problem(error.to_string())
            }
            ServicePopulateError::DockerUnavailable(ref inner) => Problem::from(inner),
            ServicePopulateError::TargetConnection { .. } => {
                problemdetails::new(StatusCode::BAD_GATEWAY)
                    .with_title("Database Server Unreachable")
                    .with_detail(error.to_string())
            }
            ServicePopulateError::ServiceConfig { .. } => {
                problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
                    .with_title("Service Configuration Error")
                    .with_detail(error.to_string())
            }
            ServicePopulateError::Database(_) => {
                problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
                    .with_title("Internal Server Error")
                    .with_detail(error.to_string())
            }
        }
    }
}

/// Copy an external PostgreSQL database into a database of this service.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PopulateServiceRequest {
    /// Destination database inside the service, e.g. the per-environment
    /// database of a project link (`<project>_<environment>`). Created when
    /// missing, owned by the service user. Must match `[a-z_][a-z0-9_]{0,62}`.
    #[schema(example = "my_app_production")]
    pub database: String,
    /// Source connection URL (`postgres://user:password@host:port/database`,
    /// optional `?sslmode=`). Must point at a publicly reachable server.
    /// Never stored, logged or returned.
    #[schema(example = "postgres://user:password@db.example.com:5432/app?sslmode=require")]
    pub source_url: String,
    /// Drop and recreate the destination when it already has tables.
    /// Without it, a non-empty destination is refused with 409.
    #[serde(default)]
    pub replace: bool,
}

/// One populate run.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PopulateRunResponse {
    pub id: i32,
    pub service_id: i32,
    pub database: String,
    /// Source URL with user and password replaced by `***`.
    pub source_url_masked: String,
    pub replace: bool,
    /// `running`, `completed` or `failed`.
    pub status: String,
    /// Client image of the transfer container (`postgres:<major>-alpine`,
    /// major version of the destination server).
    pub client_image: String,
    pub error_message: Option<String>,
    /// Size of the destination database after a successful copy.
    pub database_size_bytes: Option<i64>,
    /// ISO 8601
    pub started_at: String,
    /// ISO 8601, set once the run finished.
    pub finished_at: Option<String>,
    /// Set once the run finished.
    pub duration_seconds: Option<f64>,
    pub created_by: Option<i32>,
}

impl From<service_populate_runs::Model> for PopulateRunResponse {
    fn from(run: service_populate_runs::Model) -> Self {
        let duration_seconds = run
            .finished_at
            .map(|finished| (finished - run.started_at).num_milliseconds() as f64 / 1000.0);
        Self {
            id: run.id,
            service_id: run.service_id,
            database: run.database_name,
            source_url_masked: run.source_url_masked,
            replace: run.replace_existing,
            status: run.status,
            client_image: run.client_image,
            error_message: run.error_message,
            database_size_bytes: run.database_size_bytes,
            started_at: run
                .started_at
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            finished_at: run
                .finished_at
                .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            duration_seconds,
            created_by: run.created_by,
        }
    }
}

/// A page of populate runs.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PopulateRunListResponse {
    pub runs: Vec<PopulateRunResponse>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct PopulateRunListQuery {
    /// 1-based page (default 1)
    pub page: Option<u64>,
    /// Page size (default 20, max 100)
    pub page_size: Option<u64>,
}

/// Audit record of a populate start. Carries the masked source URL only.
#[derive(Debug, Clone, Serialize)]
pub struct ExternalServicePopulateStartedAudit {
    pub context: AuditContext,
    pub service_id: i32,
    pub run_id: i32,
    pub database: String,
    pub source_url_masked: String,
    pub replace: bool,
}

impl AuditOperation for ExternalServicePopulateStartedAudit {
    fn operation_type(&self) -> String {
        "EXTERNAL_SERVICE_POPULATE_STARTED".to_string()
    }

    fn user_id(&self) -> Option<i32> {
        Some(self.context.user_id)
    }

    fn ip_address(&self) -> Option<String> {
        self.context.ip_address.clone()
    }

    fn user_agent(&self) -> &str {
        &self.context.user_agent
    }

    fn serialize(&self) -> anyhow::Result<String> {
        serde_json::to_string(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize audit operation {}", e))
    }
}

/// Only an instance administrator, acting as themselves (not through a
/// project-scoped token), may populate or inspect populate runs.
fn require_instance_admin(auth: &AuthContext) -> Result<(), Problem> {
    if auth.is_instance_admin() && auth.project_id().is_none() && !auth.is_deployment_token() {
        return Ok(());
    }
    Err(problemdetails::new(StatusCode::FORBIDDEN)
        .with_title("Insufficient Permissions")
        .with_detail(
            "Populating a service database is restricted to an instance administrator \
             (a project-scoped or deployment token is not enough)",
        ))
}

/// Start copying an external PostgreSQL database into a database of a
/// managed PostgreSQL service. Returns at once with the run; poll
/// `GET /external-services/{id}/populate-runs/{run_id}` for the outcome.
#[utoipa::path(
    post,
    path = "/external-services/{id}/populate",
    tag = "External Services",
    params(("id" = i32, Path, description = "External service ID")),
    request_body = PopulateServiceRequest,
    responses(
        (status = 202, description = "Copy started", body = PopulateRunResponse),
        (status = 400, description = "Invalid database name or source URL", body = ProblemDetails),
        (status = 401, description = "Unauthorized", body = ProblemDetails),
        (status = 403, description = "Not an instance administrator", body = ProblemDetails),
        (status = 404, description = "Service not found", body = ProblemDetails),
        (status = 409, description = "Destination not empty (use replace) or a copy is already running", body = ProblemDetails),
        (status = 422, description = "Service is not a local standalone PostgreSQL", body = ProblemDetails),
        (status = 502, description = "Service database server unreachable", body = ProblemDetails),
        (status = 500, description = "Internal server error", body = ProblemDetails)
    ),
    security(("bearer_auth" = []))
)]
pub async fn start_service_populate(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<PopulateAppState>>,
    Extension(metadata): Extension<RequestMetadata>,
    Path(id): Path<i32>,
    Json(request): Json<PopulateServiceRequest>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ExternalServicesWrite);
    require_instance_admin(&auth)?;

    let run = state
        .populate_service
        .start(StartPopulateRequest {
            service_id: id,
            database: request.database,
            source_url: request.source_url,
            replace: request.replace,
            created_by: auth.user_id_opt(),
        })
        .await?;

    let audit = ExternalServicePopulateStartedAudit {
        context: AuditContext {
            user_id: auth.user_id(),
            ip_address: Some(metadata.ip_address.clone()),
            user_agent: metadata.user_agent.clone(),
        },
        service_id: id,
        run_id: run.id,
        database: run.database_name.clone(),
        source_url_masked: run.source_url_masked.clone(),
        replace: run.replace_existing,
    };
    if let Err(e) = state.audit_service.create_audit_log(&audit).await {
        error!(
            "Failed to create audit log for populate run {}: {}",
            run.id, e
        );
    }

    Ok((StatusCode::ACCEPTED, Json(PopulateRunResponse::from(run))))
}

/// List the populate runs of a service, newest first.
#[utoipa::path(
    get,
    path = "/external-services/{id}/populate-runs",
    tag = "External Services",
    params(("id" = i32, Path, description = "External service ID"), PopulateRunListQuery),
    responses(
        (status = 200, description = "Populate runs", body = PopulateRunListResponse),
        (status = 401, description = "Unauthorized", body = ProblemDetails),
        (status = 403, description = "Not an instance administrator", body = ProblemDetails),
        (status = 404, description = "Service not found", body = ProblemDetails),
        (status = 500, description = "Internal server error", body = ProblemDetails)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_service_populate_runs(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<PopulateAppState>>,
    Path(id): Path<i32>,
    Query(query): Query<PopulateRunListQuery>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ExternalServicesRead);
    require_instance_admin(&auth)?;

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let (runs, total) = state
        .populate_service
        .list_runs(id, Some(page), Some(page_size))
        .await?;
    Ok(Json(PopulateRunListResponse {
        runs: runs.into_iter().map(PopulateRunResponse::from).collect(),
        total,
        page,
        page_size,
    }))
}

/// Get one populate run of a service.
#[utoipa::path(
    get,
    path = "/external-services/{id}/populate-runs/{run_id}",
    tag = "External Services",
    params(
        ("id" = i32, Path, description = "External service ID"),
        ("run_id" = i32, Path, description = "Populate run ID")
    ),
    responses(
        (status = 200, description = "Populate run", body = PopulateRunResponse),
        (status = 401, description = "Unauthorized", body = ProblemDetails),
        (status = 403, description = "Not an instance administrator", body = ProblemDetails),
        (status = 404, description = "Run not found for this service", body = ProblemDetails),
        (status = 500, description = "Internal server error", body = ProblemDetails)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_service_populate_run(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<PopulateAppState>>,
    Path((id, run_id)): Path<(i32, i32)>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ExternalServicesRead);
    require_instance_admin(&auth)?;

    let run = state.populate_service.get_run(id, run_id).await?;
    Ok(Json(PopulateRunResponse::from(run)))
}

pub fn configure_routes() -> Router<Arc<PopulateAppState>> {
    Router::new()
        .route(
            "/external-services/{id}/populate",
            post(start_service_populate),
        )
        .route(
            "/external-services/{id}/populate-runs",
            get(list_service_populate_runs),
        )
        .route(
            "/external-services/{id}/populate-runs/{run_id}",
            get(get_service_populate_run),
        )
}

#[derive(OpenApi)]
#[openapi(
    paths(
        start_service_populate,
        list_service_populate_runs,
        get_service_populate_run
    ),
    components(schemas(PopulateServiceRequest, PopulateRunResponse, PopulateRunListResponse))
)]
pub struct PopulateApiDoc;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn auth_with_role(role: temps_auth::Role) -> AuthContext {
        let now = Utc::now();
        let user = temps_entities::users::Model {
            id: 42,
            name: "Populate Tester".to_string(),
            email: "tester@example.com".to_string(),
            password_hash: None,
            email_verified: true,
            email_verification_token: None,
            email_verification_expires: None,
            password_reset_token: None,
            password_reset_expires: None,
            must_change_password: false,
            deleted_at: None,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_recovery_codes: None,
            oidc_subject: None,
            oidc_provider_id: None,
            created_at: now,
            updated_at: now,
        };
        AuthContext::new_session(user, role)
    }

    #[test]
    fn only_an_instance_admin_passes_the_guard() {
        assert!(require_instance_admin(&auth_with_role(temps_auth::Role::Admin)).is_ok());
        assert!(require_instance_admin(&auth_with_role(temps_auth::Role::User)).is_err());
    }

    #[test]
    fn response_carries_duration_and_iso_dates() {
        let started = Utc::now();
        let run = service_populate_runs::Model {
            id: 1,
            service_id: 2,
            database_name: "app_homolog".to_string(),
            source_url_masked: "postgres://***:***@db.example.com:5432/app".to_string(),
            replace_existing: true,
            status: "completed".to_string(),
            client_image: "postgres:18-alpine".to_string(),
            error_message: None,
            database_size_bytes: Some(1024),
            started_at: started,
            finished_at: Some(started + Duration::milliseconds(2500)),
            created_by: Some(42),
            created_at: started,
            updated_at: started,
        };
        let response = PopulateRunResponse::from(run);
        assert_eq!(response.duration_seconds, Some(2.5));
        assert!(response.started_at.ends_with('Z'));
        assert!(response
            .finished_at
            .as_deref()
            .is_some_and(|t| t.ends_with('Z')));
        assert_eq!(response.database, "app_homolog");
        assert!(response.replace);
    }

    #[test]
    fn errors_map_to_the_documented_statuses() {
        let cases: Vec<(ServicePopulateError, u16)> = vec![
            (ServicePopulateError::ServiceNotFound { service_id: 1 }, 404),
            (
                ServicePopulateError::RunNotFound {
                    service_id: 1,
                    run_id: 2,
                },
                404,
            ),
            (
                ServicePopulateError::InvalidDatabaseName {
                    database: "X".to_string(),
                    reason: "bad".to_string(),
                },
                400,
            ),
            (
                ServicePopulateError::InvalidSourceUrl {
                    reason: "bad".to_string(),
                },
                400,
            ),
            (
                ServicePopulateError::TargetNotEmpty {
                    service_id: 1,
                    database: "db".to_string(),
                    table_count: 2,
                },
                409,
            ),
            (
                ServicePopulateError::AlreadyRunning {
                    service_id: 1,
                    database: "db".to_string(),
                },
                409,
            ),
            (
                ServicePopulateError::UnsupportedService {
                    service_id: 1,
                    reason: "redis".to_string(),
                },
                422,
            ),
            (
                ServicePopulateError::TargetConnection {
                    service_id: 1,
                    reason: "refused".to_string(),
                },
                502,
            ),
        ];
        for (error, status) in cases {
            let problem = Problem::from(error);
            assert_eq!(problem.status_code.as_u16(), status);
        }
    }
}
