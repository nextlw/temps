// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! HTTP surface for project groups (ADR-049): `/project-groups`.
//!
//! Every handler follows the same order: instance-wide permission, refuse
//! deployment tokens (a machine credential bound to one project has no
//! business with a console grouping), load the group, then apply
//! [`crate::visibility`] to what the access checker says the caller can
//! reach. A group hidden from the caller answers 404, never 403, so its
//! existence does not leak.
//!
//! The per-project checks call the registered `ProjectAccessChecker`
//! directly instead of through `project_access_guard!` and
//! `project_permission_guard!`: one batched call answers for every member
//! and the assigned project together, with the same admin bypass,
//! narrowing by the grant's role and fail-closed semantics as the macros.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::{
    extract::{rejection::JsonRejection, Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
    Json, Router,
};
use serde::Serialize;
use temps_auth::{deny_deployment_token, permission_guard, AuthContext, Permission, RequireAuth};
use temps_core::error_builder::ErrorBuilder;
use temps_core::problemdetails::{PermissionDenialKind, Problem};
use temps_core::{
    AuditContext, AuditLogger, AuditOperation, ProjectAccessChecker, RequestMetadata,
};
use utoipa::{OpenApi, ToSchema};

use crate::service::{
    CreateProjectGroupRequest, ProjectGroupService, ProjectGroupWithMembers,
    UpdateProjectGroupRequest,
};
use crate::visibility::{can_manage, visible_group, visible_groups, ServiceAccess};
use crate::ProjectGroupError;

/// Handler state shared by every `/project-groups` route.
#[derive(Clone)]
pub struct ProjectGroupsAppState {
    pub service: Arc<ProjectGroupService>,
    pub audit: Arc<dyn AuditLogger>,
    /// `None` on an instance with no access checker registered: nothing is
    /// hidden from anyone, as with `project_access_guard!`.
    pub project_access_checker: Option<Arc<dyn ProjectAccessChecker>>,
}

// ---------------------------------------------------------------------------
// Response DTO
// ---------------------------------------------------------------------------

/// A project group as the caller may see it. `service_ids` and
/// `service_count` only cover the projects the caller can reach.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ProjectGroupResponse {
    pub id: i32,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    /// Visible member project ids, ascending.
    pub service_ids: Vec<i32>,
    /// Always `service_ids.len()`.
    pub service_count: i64,
    /// Unix epoch milliseconds.
    #[schema(example = 1790812800000_i64)]
    pub created_at: i64,
    /// Unix epoch milliseconds.
    #[schema(example = 1790812860000_i64)]
    pub updated_at: i64,
}

impl From<ProjectGroupWithMembers> for ProjectGroupResponse {
    fn from(value: ProjectGroupWithMembers) -> Self {
        let group = value.group;
        Self {
            id: group.id,
            slug: group.slug,
            name: group.name,
            description: group.description,
            service_count: value.project_ids.len() as i64,
            service_ids: value.project_ids,
            created_at: group.created_at.timestamp_millis(),
            updated_at: group.updated_at.timestamp_millis(),
        }
    }
}

// ---------------------------------------------------------------------------
// Audit events
// ---------------------------------------------------------------------------

macro_rules! audit_operation {
    ($ty:ident, $name:literal) => {
        impl AuditOperation for $ty {
            fn operation_type(&self) -> String {
                $name.to_string()
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
                    .map_err(|e| anyhow::anyhow!("failed to serialize {}: {e}", $name))
            }
        }
    };
}

#[derive(Debug, Clone, Serialize)]
struct ProjectGroupCreatedAudit {
    #[serde(flatten)]
    context: AuditContext,
    group_id: i32,
    name: String,
    slug: String,
}
audit_operation!(ProjectGroupCreatedAudit, "PROJECT_GROUP_CREATED");

#[derive(Debug, Clone, Serialize)]
struct ProjectGroupUpdatedAudit {
    #[serde(flatten)]
    context: AuditContext,
    group_id: i32,
    /// New name, only when it changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Whether the description changed (its text is not copied into the
    /// audit log).
    description_changed: bool,
}
audit_operation!(ProjectGroupUpdatedAudit, "PROJECT_GROUP_UPDATED");

#[derive(Debug, Clone, Serialize)]
struct ProjectGroupDeletedAudit {
    #[serde(flatten)]
    context: AuditContext,
    group_id: i32,
    name: String,
    slug: String,
}
audit_operation!(ProjectGroupDeletedAudit, "PROJECT_GROUP_DELETED");

#[derive(Debug, Clone, Serialize)]
struct ProjectGroupProjectAssignedAudit {
    #[serde(flatten)]
    context: AuditContext,
    group_id: i32,
    project_id: i32,
    previous_group_id: Option<i32>,
}
audit_operation!(
    ProjectGroupProjectAssignedAudit,
    "PROJECT_GROUP_PROJECT_ASSIGNED"
);

#[derive(Debug, Clone, Serialize)]
struct ProjectGroupProjectRemovedAudit {
    #[serde(flatten)]
    context: AuditContext,
    group_id: i32,
    project_id: i32,
}
audit_operation!(
    ProjectGroupProjectRemovedAudit,
    "PROJECT_GROUP_PROJECT_REMOVED"
);

fn audit_context(auth: &AuthContext, metadata: &RequestMetadata) -> AuditContext {
    AuditContext {
        user_id: auth.user_id(),
        ip_address: Some(metadata.ip_address.clone()),
        user_agent: metadata.user_agent.clone(),
    }
}

/// Audit failures are logged, not returned: the change already happened,
/// and failing the request would invite a retry of something that worked.
async fn record(state: &ProjectGroupsAppState, operation: &dyn AuditOperation) {
    if let Err(e) = state.audit.create_audit_log(operation).await {
        tracing::error!(
            error = %e,
            operation = %operation.operation_type(),
            "project groups: failed to write audit log"
        );
    }
}

// ---------------------------------------------------------------------------
// Access helpers
// ---------------------------------------------------------------------------

/// What the caller can reach among `project_ids`.
///
/// Instance administrators and instances without a checker see everything.
/// Any checker failure fails the request (500): an unanswerable check must
/// never widen what a user sees.
async fn resolve_access(
    state: &ProjectGroupsAppState,
    auth: &AuthContext,
    project_ids: &[i32],
) -> Result<ServiceAccess, Problem> {
    if auth.is_instance_admin() {
        return Ok(ServiceAccess::Unrestricted);
    }
    let Some(checker) = state.project_access_checker.as_ref() else {
        return Ok(ServiceAccess::Unrestricted);
    };
    // Fail closed: deployment tokens are refused before this point, and
    // "no identity" must never resolve to "sees everything".
    let Some(user_id) = auth.user_id_opt() else {
        tracing::error!("project groups: authenticated caller has no user id");
        return Err(access_denied("Could not resolve caller identity"));
    };
    if project_ids.is_empty() {
        return Ok(ServiceAccess::Only(BTreeSet::new()));
    }
    let answers = match checker.user_can_access_projects(user_id, project_ids).await {
        Ok(answers) => answers,
        Err(e) => {
            tracing::error!(
                user_id,
                error = %e,
                "project groups: ProjectAccessChecker infrastructure failure — denying"
            );
            return Err(ErrorBuilder::new(StatusCode::INTERNAL_SERVER_ERROR)
                .type_("https://temps.sh/probs/project-access-check-failed")
                .title("Project Access Check Failed")
                .detail("Could not verify project access; please try again")
                .build());
        }
    };
    // An id the checker left out of its answer counts as denied.
    let allowed = answers
        .into_iter()
        .filter_map(|(id, allowed)| allowed.then_some(id))
        .collect();
    Ok(ServiceAccess::Only(allowed))
}

fn access_denied(detail: &str) -> Problem {
    ErrorBuilder::new(StatusCode::FORBIDDEN)
        .type_("https://temps.sh/probs/project-access-denied")
        .title("Project Access Denied")
        .detail(detail)
        .permission_denial(PermissionDenialKind::ProjectAccess, None)
        .build()
}

fn hidden_member_denied() -> Problem {
    access_denied("This project includes services you do not have access to")
}

fn project_access_denied() -> Problem {
    access_denied("Your team membership does not include access to this project")
}

/// The group as the caller sees it, or the same 404 a missing group gets.
fn visible_or_not_found(
    group: ProjectGroupWithMembers,
    access: &ServiceAccess,
) -> Result<ProjectGroupWithMembers, Problem> {
    let group_id = group.group.id;
    visible_group(group, access).ok_or_else(|| ProjectGroupError::NotFound { group_id }.into())
}

/// Like [`visible_or_not_found`], for a group loaded by slug.
fn visible_by_slug_or_not_found(
    group: ProjectGroupWithMembers,
    access: &ServiceAccess,
) -> Result<ProjectGroupWithMembers, Problem> {
    let slug = group.group.slug.clone();
    visible_group(group, access).ok_or_else(|| ProjectGroupError::NotFoundBySlug { slug }.into())
}

/// Strips hidden ids from a group returned after a mutation. Membership can
/// change between the access check and the write, so the response is
/// filtered again rather than trusted.
fn without_hidden(
    mut group: ProjectGroupWithMembers,
    access: &ServiceAccess,
) -> ProjectGroupWithMembers {
    group.project_ids.retain(|id| access.can_access(*id));
    group
}

/// Narrows a mutation by the caller's role on every project it touches, as
/// `project_permission_guard!` does for project mutations: a grant whose
/// role lacks `permission` (a `viewer`, say) refuses the change even though
/// the instance-wide role allows it.
///
/// `None` from the checker means it has no per-permission opinion on that
/// project; the coarse answer already in `access` decides, as the macro
/// falls back to `user_can_access_project`. A project missing from the
/// batch answer is refused rather than guessed.
async fn require_project_permission(
    state: &ProjectGroupsAppState,
    auth: &AuthContext,
    access: &ServiceAccess,
    project_ids: &[i32],
    permission: Permission,
) -> Result<(), Problem> {
    if auth.is_instance_admin() || project_ids.is_empty() {
        return Ok(());
    }
    let Some(checker) = state.project_access_checker.as_ref() else {
        return Ok(());
    };
    let Some(user_id) = auth.user_id_opt() else {
        tracing::error!("project groups: authenticated caller has no user id");
        return Err(access_denied("Could not resolve caller identity"));
    };
    let answers = match checker
        .effective_project_permissions_batch(user_id, project_ids)
        .await
    {
        Ok(answers) => answers,
        Err(e) => {
            tracing::error!(
                user_id,
                error = %e,
                "project groups: effective_project_permissions_batch failed — denying"
            );
            return Err(ErrorBuilder::new(StatusCode::INTERNAL_SERVER_ERROR)
                .type_("https://temps.sh/probs/project-permission-check-failed")
                .title("Project Permission Check Failed")
                .detail("Could not verify project permissions; please try again")
                .build());
        }
    };
    let required = permission.to_string();
    for project_id in project_ids {
        let allowed = match answers.get(project_id) {
            Some(Some(held)) => held.contains(&required),
            Some(None) => access.can_access(*project_id),
            None => false,
        };
        if !allowed {
            return Err(project_permission_denied(&required));
        }
    }
    Ok(())
}

fn project_permission_denied(required: &str) -> Problem {
    ErrorBuilder::new(StatusCode::FORBIDDEN)
        .type_("https://temps.sh/probs/project-permission-denied")
        .title("Project Permission Denied")
        .detail(format!(
            "Your role on a service in this request does not include the {required} permission"
        ))
        .value("required_permission", required)
        .permission_denial(
            PermissionDenialKind::ProjectPermission,
            Some(required.to_string()),
        )
        .build()
}

/// A malformed or unknown-field body is the caller's mistake: 400 with a
/// problem body, like the service's own validation errors, rather than
/// axum's plain-text 422.
fn json_body<T>(payload: Result<Json<T>, JsonRejection>) -> Result<T, Problem> {
    let Json(body) = payload.map_err(|rejection| ProjectGroupError::Validation {
        message: format!("Invalid request body: {}", rejection.body_text()),
    })?;
    Ok(body)
}

fn with_project(ids: &[i32], project_id: i32) -> Vec<i32> {
    let mut all = ids.to_vec();
    all.push(project_id);
    all
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[utoipa::path(
    tag = "Project Groups",
    get,
    path = "/project-groups",
    responses(
        (status = 200, description = "Visible project groups, ordered by name", body = Vec<ProjectGroupResponse>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Insufficient permissions or deployment token"),
        (status = 500, description = "Access check or database failure"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_project_groups(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<ProjectGroupsAppState>>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ProjectsRead);
    deny_deployment_token!(auth);

    let groups = state.service.list().await?;
    let member_ids: Vec<i32> = groups
        .iter()
        .flat_map(|g| g.project_ids.iter().copied())
        .collect();
    let access = resolve_access(&state, &auth, &member_ids).await?;
    let visible: Vec<ProjectGroupResponse> = visible_groups(groups, &access)
        .into_iter()
        .map(ProjectGroupResponse::from)
        .collect();
    Ok(Json(visible))
}

#[utoipa::path(
    tag = "Project Groups",
    post,
    path = "/project-groups",
    request_body = CreateProjectGroupRequest,
    responses(
        (status = 201, description = "Project group created", body = ProjectGroupResponse),
        (status = 400, description = "Empty or over-long name, invalid slug, or malformed body (unknown fields included)"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Insufficient permissions or deployment token"),
        (status = 409, description = "Slug already taken"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_project_group(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<ProjectGroupsAppState>>,
    Extension(metadata): Extension<RequestMetadata>,
    payload: Result<Json<CreateProjectGroupRequest>, JsonRejection>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ProjectsCreate);
    deny_deployment_token!(auth);
    let req = json_body(payload)?;

    let created = state.service.create(req).await?;
    record(
        &state,
        &ProjectGroupCreatedAudit {
            context: audit_context(&auth, &metadata),
            group_id: created.group.id,
            name: created.group.name.clone(),
            slug: created.group.slug.clone(),
        },
    )
    .await;
    let body = Json(ProjectGroupResponse::from(created));
    Ok((StatusCode::CREATED, body))
}

#[utoipa::path(
    tag = "Project Groups",
    get,
    path = "/project-groups/{id}",
    params(("id" = i32, Path, description = "Project group ID")),
    responses(
        (status = 200, description = "Project group", body = ProjectGroupResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Insufficient permissions or deployment token"),
        (status = 404, description = "Not found, or every service in it is hidden from the caller"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_project_group(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<ProjectGroupsAppState>>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ProjectsRead);
    deny_deployment_token!(auth);

    let group = state.service.get(id).await?;
    let access = resolve_access(&state, &auth, &group.project_ids).await?;
    let group = visible_or_not_found(group, &access)?;
    Ok(Json(ProjectGroupResponse::from(group)))
}

#[utoipa::path(
    tag = "Project Groups",
    get,
    path = "/project-groups/by-slug/{slug}",
    params(("slug" = String, Path, description = "Project group slug")),
    responses(
        (status = 200, description = "Project group", body = ProjectGroupResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Insufficient permissions or deployment token"),
        (status = 404, description = "Not found, or every service in it is hidden from the caller"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_project_group_by_slug(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<ProjectGroupsAppState>>,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ProjectsRead);
    deny_deployment_token!(auth);

    let group = state.service.get_by_slug(&slug).await?;
    let access = resolve_access(&state, &auth, &group.project_ids).await?;
    let group = visible_by_slug_or_not_found(group, &access)?;
    Ok(Json(ProjectGroupResponse::from(group)))
}

#[utoipa::path(
    tag = "Project Groups",
    patch,
    path = "/project-groups/{id}",
    params(("id" = i32, Path, description = "Project group ID")),
    request_body = UpdateProjectGroupRequest,
    responses(
        (status = 200, description = "Project group updated", body = ProjectGroupResponse),
        (status = 400, description = "Empty or over-long name, or malformed body (unknown fields such as slug included)"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Insufficient permissions, deployment token, a service in the group is hidden from the caller, or the caller's role on one of its services lacks projects:write"),
        (status = 404, description = "Not found, or every service in it is hidden from the caller"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_project_group(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<ProjectGroupsAppState>>,
    Extension(metadata): Extension<RequestMetadata>,
    Path(id): Path<i32>,
    payload: Result<Json<UpdateProjectGroupRequest>, JsonRejection>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ProjectsWrite);
    deny_deployment_token!(auth);
    let req = json_body(payload)?;

    let group = state.service.get(id).await?;
    let access = resolve_access(&state, &auth, &group.project_ids).await?;
    if !can_manage(&group, &access) {
        visible_or_not_found(group, &access)?;
        return Err(hidden_member_denied());
    }
    let members = &group.project_ids;
    require_project_permission(&state, &auth, &access, members, Permission::ProjectsWrite).await?;

    let (updated, changed) = state.service.update(id, req).await?;
    if changed.any() {
        record(
            &state,
            &ProjectGroupUpdatedAudit {
                context: audit_context(&auth, &metadata),
                group_id: id,
                name: changed.name.then(|| updated.group.name.clone()),
                description_changed: changed.description,
            },
        )
        .await;
    }
    let updated = without_hidden(updated, &access);
    Ok(Json(ProjectGroupResponse::from(updated)))
}

#[utoipa::path(
    tag = "Project Groups",
    delete,
    path = "/project-groups/{id}",
    params(("id" = i32, Path, description = "Project group ID")),
    responses(
        (status = 204, description = "Project group deleted; its services are kept, ungrouped"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Insufficient permissions, deployment token, a service in the group is hidden from the caller, or the caller's role on one of its services lacks projects:delete"),
        (status = 404, description = "Not found, or every service in it is hidden from the caller"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_project_group(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<ProjectGroupsAppState>>,
    Extension(metadata): Extension<RequestMetadata>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ProjectsDelete);
    deny_deployment_token!(auth);

    let group = state.service.get(id).await?;
    let access = resolve_access(&state, &auth, &group.project_ids).await?;
    if !can_manage(&group, &access) {
        visible_or_not_found(group, &access)?;
        return Err(hidden_member_denied());
    }
    let members = &group.project_ids;
    require_project_permission(&state, &auth, &access, members, Permission::ProjectsDelete).await?;

    state.service.delete(id).await?;
    record(
        &state,
        &ProjectGroupDeletedAudit {
            context: audit_context(&auth, &metadata),
            group_id: id,
            name: group.group.name,
            slug: group.group.slug,
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    tag = "Project Groups",
    put,
    path = "/project-groups/{id}/projects/{project_id}",
    params(
        ("id" = i32, Path, description = "Project group ID"),
        ("project_id" = i32, Path, description = "Project (service) ID"),
    ),
    responses(
        (status = 200, description = "Service is in the group (moved from another group if needed)", body = ProjectGroupResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Insufficient permissions, deployment token, no access to the service, or the caller's role on it lacks projects:write"),
        (status = 404, description = "Group or service not found, or the group is hidden from the caller"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn assign_project_to_group(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<ProjectGroupsAppState>>,
    Extension(metadata): Extension<RequestMetadata>,
    Path((id, project_id)): Path<(i32, i32)>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ProjectsWrite);
    deny_deployment_token!(auth);

    let group = state.service.get(id).await?;
    let candidates = with_project(&group.project_ids, project_id);
    let access = resolve_access(&state, &auth, &candidates).await?;
    visible_or_not_found(group, &access)?;
    // Without this a caller could pull a service they are gated out of into
    // a group they control.
    if !access.can_access(project_id) {
        return Err(project_access_denied());
    }
    let target = [project_id];
    require_project_permission(&state, &auth, &access, &target, Permission::ProjectsWrite).await?;

    let outcome = state.service.assign(id, project_id).await?;
    if outcome.changed {
        record(
            &state,
            &ProjectGroupProjectAssignedAudit {
                context: audit_context(&auth, &metadata),
                group_id: id,
                project_id,
                previous_group_id: outcome.previous_group_id,
            },
        )
        .await;
    }
    let group = without_hidden(outcome.group, &access);
    Ok(Json(ProjectGroupResponse::from(group)))
}

#[utoipa::path(
    tag = "Project Groups",
    delete,
    path = "/project-groups/{id}/projects/{project_id}",
    params(
        ("id" = i32, Path, description = "Project group ID"),
        ("project_id" = i32, Path, description = "Project (service) ID"),
    ),
    responses(
        (status = 204, description = "Service removed from the group; it is kept, ungrouped"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Insufficient permissions, deployment token, no access to the service, or the caller's role on it lacks projects:write"),
        (status = 404, description = "Group not found or hidden, or the service is not in this group"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn remove_project_from_group(
    RequireAuth(auth): RequireAuth,
    State(state): State<Arc<ProjectGroupsAppState>>,
    Extension(metadata): Extension<RequestMetadata>,
    Path((id, project_id)): Path<(i32, i32)>,
) -> Result<impl IntoResponse, Problem> {
    permission_guard!(auth, ProjectsWrite);
    deny_deployment_token!(auth);

    let group = state.service.get(id).await?;
    let candidates = with_project(&group.project_ids, project_id);
    let access = resolve_access(&state, &auth, &candidates).await?;
    visible_or_not_found(group, &access)?;
    if !access.can_access(project_id) {
        return Err(project_access_denied());
    }
    let target = [project_id];
    require_project_permission(&state, &auth, &access, &target, Permission::ProjectsWrite).await?;

    state.service.unassign(id, project_id).await?;
    record(
        &state,
        &ProjectGroupProjectRemovedAudit {
            context: audit_context(&auth, &metadata),
            group_id: id,
            project_id,
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router(state: Arc<ProjectGroupsAppState>) -> Router {
    Router::new()
        .route(
            "/project-groups",
            get(list_project_groups).post(create_project_group),
        )
        .route(
            "/project-groups/by-slug/{slug}",
            get(get_project_group_by_slug),
        )
        .route(
            "/project-groups/{id}",
            get(get_project_group)
                .patch(update_project_group)
                .delete(delete_project_group),
        )
        .route(
            "/project-groups/{id}/projects/{project_id}",
            put(assign_project_to_group).delete(remove_project_from_group),
        )
        .with_state(state)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        list_project_groups,
        create_project_group,
        get_project_group,
        get_project_group_by_slug,
        update_project_group,
        delete_project_group,
        assign_project_to_group,
        remove_project_from_group,
    ),
    components(schemas(
        ProjectGroupResponse,
        CreateProjectGroupRequest,
        UpdateProjectGroupRequest,
    )),
    tags((
        name = "Project Groups",
        description = "Groups of projects, shown as Projects in the console (ADR-049)"
    ))
)]
pub struct ProjectGroupsApiDoc;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use axum::http::HeaderMap;
    use chrono::Utc;
    use sea_orm::{DatabaseBackend, MockDatabase, MockExecResult, Value};
    use temps_auth::{Permission, Role};
    use temps_entities::deployment_tokens::DeploymentTokenPermission;
    use temps_entities::{project_group_members, project_groups, users};

    /// Answers from a fixed allow-list, or fails every call.
    ///
    /// `permissions` plays the grant's role: a project absent from it has no
    /// per-permission opinion (`None`), as on an ungated project.
    #[derive(Default)]
    struct FakeChecker {
        allowed: BTreeSet<i32>,
        permissions: BTreeMap<i32, Vec<String>>,
        /// Left out of the permission batch answer, breaking its contract.
        omitted: BTreeSet<i32>,
        fail: bool,
        fail_permissions: bool,
    }

    type CheckerError = Box<dyn std::error::Error + Send + Sync>;

    #[temps_core::async_trait::async_trait]
    impl ProjectAccessChecker for FakeChecker {
        async fn user_can_access_project(
            &self,
            _user_id: i32,
            project_id: i32,
        ) -> Result<bool, CheckerError> {
            if self.fail {
                return Err("checker unavailable".into());
            }
            Ok(self.allowed.contains(&project_id))
        }

        async fn user_can_access_projects(
            &self,
            _user_id: i32,
            project_ids: &[i32],
        ) -> Result<BTreeMap<i32, bool>, CheckerError> {
            if self.fail {
                return Err("checker unavailable".into());
            }
            Ok(project_ids
                .iter()
                .map(|id| (*id, self.allowed.contains(id)))
                .collect())
        }

        async fn effective_project_permissions_batch(
            &self,
            _user_id: i32,
            project_ids: &[i32],
        ) -> Result<BTreeMap<i32, Option<Vec<String>>>, CheckerError> {
            if self.fail_permissions {
                return Err("permission resolver unavailable".into());
            }
            Ok(project_ids
                .iter()
                .filter(|id| !self.omitted.contains(id))
                .map(|id| (*id, self.permissions.get(id).cloned()))
                .collect())
        }
    }

    #[derive(Default)]
    struct RecordingAudit {
        operations: Mutex<Vec<(String, String)>>,
    }

    #[temps_core::async_trait::async_trait]
    impl AuditLogger for RecordingAudit {
        async fn create_audit_log(&self, operation: &dyn AuditOperation) -> anyhow::Result<()> {
            let entry = (operation.operation_type(), operation.serialize()?);
            self.operations
                .lock()
                .map_err(|_| anyhow::anyhow!("audit mutex poisoned"))?
                .push(entry);
            Ok(())
        }
    }

    impl RecordingAudit {
        fn recorded(&self) -> Vec<(String, String)> {
            self.operations.lock().expect("audit mutex").clone()
        }
    }

    struct Harness {
        state: Arc<ProjectGroupsAppState>,
        audit: Arc<RecordingAudit>,
    }

    fn harness(db: MockDatabase, checker: Option<FakeChecker>) -> Harness {
        let audit = Arc::new(RecordingAudit::default());
        let checker = checker.map(|c| Arc::new(c) as Arc<dyn ProjectAccessChecker>);
        let state = Arc::new(ProjectGroupsAppState {
            service: Arc::new(ProjectGroupService::new(Arc::new(db.into_connection()))),
            audit: audit.clone(),
            project_access_checker: checker,
        });
        Harness { state, audit }
    }

    fn allow(ids: &[i32]) -> Option<FakeChecker> {
        Some(FakeChecker {
            allowed: ids.iter().copied().collect(),
            ..FakeChecker::default()
        })
    }

    const VIEWER: &[Permission] = &[Permission::ProjectsRead];
    const EDITOR: &[Permission] = &[
        Permission::ProjectsRead,
        Permission::ProjectsWrite,
        Permission::ProjectsDelete,
    ];

    /// Access to `ids`, each granted with the permissions of `role`.
    fn granted(ids: &[i32], role: &[Permission]) -> Option<FakeChecker> {
        let held: Vec<String> = role.iter().map(|p| p.to_string()).collect();
        Some(FakeChecker {
            allowed: ids.iter().copied().collect(),
            permissions: ids.iter().map(|id| (*id, held.clone())).collect(),
            ..FakeChecker::default()
        })
    }

    fn user() -> users::Model {
        let now = Utc::now();
        users::Model {
            id: 42,
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
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
        }
    }

    fn with_permissions(permissions: Vec<Permission>) -> AuthContext {
        AuthContext::new_api_key(user(), None, Some(permissions), "test".into(), 1)
    }

    fn member_user() -> AuthContext {
        with_permissions(vec![
            Permission::ProjectsRead,
            Permission::ProjectsCreate,
            Permission::ProjectsWrite,
            Permission::ProjectsDelete,
        ])
    }

    fn admin() -> AuthContext {
        AuthContext::new_session(user(), Role::Admin)
    }

    fn metadata() -> RequestMetadata {
        RequestMetadata {
            ip_address: "127.0.0.1".into(),
            user_agent: "test".into(),
            headers: HeaderMap::new(),
            visitor_id_cookie: None,
            session_id_cookie: None,
            base_url: "http://localhost".into(),
            scheme: "http".into(),
            host: "localhost".into(),
            is_secure: false,
        }
    }

    fn group_row(id: i32, name: &str) -> project_groups::Model {
        let now = Utc::now();
        project_groups::Model {
            id,
            name: name.into(),
            slug: name.to_lowercase(),
            description: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn member(project_id: i32, group_id: i32) -> project_group_members::Model {
        project_group_members::Model {
            project_id,
            group_id,
        }
    }

    fn no_members() -> Vec<project_group_members::Model> {
        Vec::new()
    }

    fn exec(rows_affected: u64) -> MockExecResult {
        MockExecResult {
            last_insert_id: 0,
            rows_affected,
        }
    }

    async fn json_of(response: impl IntoResponse) -> (StatusCode, serde_json::Value) {
        let response = response.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body is readable");
        let body = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("body is JSON")
        };
        (status, body)
    }

    fn status_of<T>(result: Result<T, Problem>) -> StatusCode {
        match result {
            Ok(_) => StatusCode::OK,
            Err(problem) => problem.status_code,
        }
    }

    #[tokio::test]
    async fn list_drops_fully_hidden_groups_and_strips_hidden_ids() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![
                group_row(1, "Back"),
                group_row(2, "Gated"),
                group_row(3, "Empty"),
            ]])
            .append_query_results([vec![member(10, 1), member(11, 1), member(20, 2)]]);
        let h = harness(db, allow(&[10]));
        let response = list_project_groups(RequireAuth(member_user()), State(h.state))
            .await
            .expect("list succeeds");
        let (status, body) = json_of(response).await;
        assert_eq!(status, StatusCode::OK);
        let ids: Vec<i64> = body
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|g| g["id"].as_i64())
            .collect();
        assert_eq!(ids, vec![1, 3]);
        assert_eq!(body[0]["service_ids"], serde_json::json!([10]));
        assert_eq!(body[0]["service_count"], 1);
        assert_eq!(body[1]["service_count"], 0);
    }

    #[tokio::test]
    async fn admin_sees_everything_without_consulting_the_checker() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(2, "Gated")]])
            .append_query_results([vec![member(20, 2)]]);
        let failing = Some(FakeChecker {
            fail: true,
            ..FakeChecker::default()
        });
        let h = harness(db, failing);
        let response = list_project_groups(RequireAuth(admin()), State(h.state))
            .await
            .expect("admin bypasses the checker");
        let (_, body) = json_of(response).await;
        assert_eq!(body[0]["service_ids"], serde_json::json!([20]));
    }

    #[tokio::test]
    async fn checker_failure_fails_the_list_closed() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(2, "Gated")]])
            .append_query_results([vec![member(20, 2)]]);
        let failing = Some(FakeChecker {
            fail: true,
            ..FakeChecker::default()
        });
        let h = harness(db, failing);
        let result = list_project_groups(RequireAuth(member_user()), State(h.state)).await;
        assert_eq!(status_of(result), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn deployment_tokens_are_refused() {
        let token = AuthContext::new_deployment_token(
            10,
            None,
            None,
            1,
            "ci".into(),
            vec![DeploymentTokenPermission::FullAccess],
        );
        let h = harness(MockDatabase::new(DatabaseBackend::Postgres), allow(&[10]));
        let result = list_project_groups(RequireAuth(token), State(h.state)).await;
        assert_eq!(status_of(result), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn get_of_a_fully_hidden_group_is_not_found() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(2, "Gated")]])
            .append_query_results([vec![member(20, 2)]]);
        let h = harness(db, allow(&[]));
        let result = get_project_group(RequireAuth(member_user()), State(h.state), Path(2)).await;
        assert_eq!(status_of(result), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_by_slug_returns_the_response_shape() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "Back")]])
            .append_query_results([vec![member(10, 1), member(12, 1)]]);
        let h = harness(db, None);
        let response = get_project_group_by_slug(
            RequireAuth(member_user()),
            State(h.state),
            Path("back".to_string()),
        )
        .await
        .expect("get by slug succeeds");
        let (_, body) = json_of(response).await;
        let keys: BTreeSet<&str> = body
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        let expected = [
            "created_at",
            "description",
            "id",
            "name",
            "service_count",
            "service_ids",
            "slug",
            "updated_at",
        ];
        assert_eq!(keys, expected.into_iter().collect());
        assert!(body["description"].is_null());
        assert!(body["created_at"].is_i64());
        assert_eq!(body["service_ids"], serde_json::json!([10, 12]));
    }

    #[tokio::test]
    async fn create_needs_projects_create() {
        let h = harness(MockDatabase::new(DatabaseBackend::Postgres), None);
        let reader = with_permissions(vec![Permission::ProjectsRead]);
        let result = create_project_group(
            RequireAuth(reader),
            State(h.state),
            Extension(metadata()),
            Ok(Json(CreateProjectGroupRequest {
                name: "CRM".into(),
                slug: None,
                description: None,
            })),
        )
        .await;
        assert_eq!(status_of(result), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn create_returns_201_and_records_the_audit_entry() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(3, "CRM")]]);
        let h = harness(db, allow(&[]));
        let response = create_project_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Ok(Json(CreateProjectGroupRequest {
                name: "CRM".into(),
                slug: None,
                description: None,
            })),
        )
        .await
        .expect("create succeeds");
        let (status, body) = json_of(response).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["slug"], "crm");
        let recorded = h.audit.recorded();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "PROJECT_GROUP_CREATED");
        let entry = &recorded[0].1;
        assert!(entry.contains(r#""group_id":3"#), "{entry}");
    }

    #[tokio::test]
    async fn update_with_a_hidden_member_is_forbidden_and_writes_nothing() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "Back")]])
            .append_query_results([vec![member(10, 1), member(11, 1)]]);
        let h = harness(db, allow(&[10]));
        let result = update_project_group(
            RequireAuth(member_user()),
            State(h.state.clone()),
            Extension(metadata()),
            Path(1),
            rename(),
        )
        .await;
        assert_eq!(status_of(result), StatusCode::FORBIDDEN);
        assert!(h.audit.recorded().is_empty());
    }

    #[tokio::test]
    async fn delete_with_a_hidden_member_is_forbidden() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "Back")]])
            .append_query_results([vec![member(10, 1), member(11, 1)]]);
        let h = harness(db, allow(&[10]));
        let result = delete_project_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path(1),
        )
        .await;
        assert_eq!(status_of(result), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn delete_of_an_empty_group_succeeds_and_is_audited() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "Back")]])
            .append_query_results([no_members()])
            .append_exec_results([exec(1)]);
        let h = harness(db, allow(&[]));
        let response = delete_project_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path(1),
        )
        .await
        .expect("delete succeeds");
        let (status, _) = json_of(response).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(h.audit.recorded()[0].0, "PROJECT_GROUP_DELETED");
    }

    #[tokio::test]
    async fn assign_without_access_to_the_service_is_forbidden() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "Back")]])
            .append_query_results([no_members()]);
        let h = harness(db, allow(&[]));
        let result = assign_project_to_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path((1, 41)),
        )
        .await;
        assert_eq!(status_of(result), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn assign_moves_the_service_and_audits_the_previous_group() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            // Handler: the target group and its members.
            .append_query_results([vec![group_row(2, "Ops")]])
            .append_query_results([no_members()])
            // Service: group, live project, current membership (group 1).
            .append_query_results([vec![group_row(2, "Ops")]])
            .append_query_results([vec![BTreeMap::from([("id", Value::from(41))])]])
            .append_query_results([vec![member(41, 1)]])
            .append_exec_results([exec(1), exec(1), exec(1)])
            .append_query_results([vec![group_row(2, "Ops")]])
            .append_query_results([vec![member(41, 2)]]);
        let h = harness(db, allow(&[41]));
        let response = assign_project_to_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path((2, 41)),
        )
        .await
        .expect("assign succeeds");
        let (status, body) = json_of(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["service_ids"], serde_json::json!([41]));
        let recorded = h.audit.recorded();
        assert_eq!(recorded[0].0, "PROJECT_GROUP_PROJECT_ASSIGNED");
        let entry = &recorded[0].1;
        assert!(entry.contains(r#""previous_group_id":1"#), "{entry}");
    }

    #[tokio::test]
    async fn assign_to_the_current_group_writes_no_audit_entry() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "Back")]])
            .append_query_results([vec![member(41, 1)]])
            .append_query_results([vec![group_row(1, "Back")]])
            .append_query_results([vec![BTreeMap::from([("id", Value::from(41))])]])
            .append_query_results([vec![member(41, 1)]])
            .append_query_results([vec![member(41, 1)]]);
        let h = harness(db, allow(&[41]));
        let response = assign_project_to_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path((1, 41)),
        )
        .await
        .expect("assign succeeds");
        let (status, _) = json_of(response).await;
        assert_eq!(status, StatusCode::OK);
        assert!(h.audit.recorded().is_empty());
    }

    #[tokio::test]
    async fn unassign_of_a_non_member_is_not_found() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "Back")]])
            .append_query_results([no_members()])
            .append_exec_results([exec(0)]);
        let h = harness(db, allow(&[41]));
        let result = remove_project_from_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path((1, 41)),
        )
        .await;
        assert_eq!(status_of(result), StatusCode::NOT_FOUND);
    }

    fn rename() -> Result<Json<UpdateProjectGroupRequest>, JsonRejection> {
        Ok(Json(UpdateProjectGroupRequest {
            name: Some("Renamed".into()),
            description: None,
        }))
    }

    /// The handler's own reads: the group, then its members.
    fn group_with_members(id: i32, members: &[i32]) -> MockDatabase {
        let rows: Vec<_> = members.iter().map(|p| member(*p, id)).collect();
        MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(id, "Back")]])
            .append_query_results([rows])
    }

    #[tokio::test]
    async fn update_by_a_viewer_of_a_member_is_forbidden() {
        let h = harness(group_with_members(1, &[10]), granted(&[10], VIEWER));
        let result = update_project_group(
            RequireAuth(member_user()),
            State(h.state.clone()),
            Extension(metadata()),
            Path(1),
            rename(),
        )
        .await;
        let problem = result.err().expect("a viewer cannot rename");
        assert_eq!(problem.status_code, StatusCode::FORBIDDEN);
        assert_eq!(problem.body["title"], "Project Permission Denied");
        assert!(h.audit.recorded().is_empty());
    }

    #[tokio::test]
    async fn update_by_an_editor_of_every_member_succeeds() {
        let db = group_with_members(1, &[10])
            // Service: load, UPDATE ... RETURNING, members.
            .append_query_results([vec![group_row(1, "Back")]])
            .append_query_results([vec![group_row(1, "Renamed")]])
            .append_query_results([vec![member(10, 1)]]);
        let h = harness(db, granted(&[10], EDITOR));
        let response = update_project_group(
            RequireAuth(member_user()),
            State(h.state.clone()),
            Extension(metadata()),
            Path(1),
            rename(),
        )
        .await
        .expect("an editor can rename");
        let (status, body) = json_of(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["name"], "Renamed");
        assert_eq!(h.audit.recorded()[0].0, "PROJECT_GROUP_UPDATED");
    }

    #[tokio::test]
    async fn delete_needs_projects_delete_on_every_member() {
        let write_only = [Permission::ProjectsRead, Permission::ProjectsWrite];
        let h = harness(group_with_members(1, &[10]), granted(&[10], &write_only));
        let result = delete_project_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path(1),
        )
        .await;
        assert_eq!(status_of(result), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_bypasses_the_grant_role() {
        let db = group_with_members(1, &[10]).append_exec_results([exec(1)]);
        let h = harness(db, granted(&[10], &[]));
        let response = delete_project_group(
            RequireAuth(admin()),
            State(h.state),
            Extension(metadata()),
            Path(1),
        )
        .await
        .expect("admins are not narrowed by grants");
        let (status, _) = json_of(response).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn assign_by_a_viewer_of_the_service_is_forbidden() {
        let h = harness(group_with_members(1, &[]), granted(&[41], VIEWER));
        let result = assign_project_to_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path((1, 41)),
        )
        .await;
        assert_eq!(status_of(result), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn unassign_by_a_viewer_of_the_service_is_forbidden() {
        let h = harness(group_with_members(1, &[41]), granted(&[41], VIEWER));
        let result = remove_project_from_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path((1, 41)),
        )
        .await;
        assert_eq!(status_of(result), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn permission_resolver_failure_fails_closed() {
        let checker = Some(FakeChecker {
            allowed: BTreeSet::from([41]),
            fail_permissions: true,
            ..FakeChecker::default()
        });
        let h = harness(group_with_members(1, &[]), checker);
        let result = assign_project_to_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path((1, 41)),
        )
        .await;
        assert_eq!(status_of(result), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn a_project_missing_from_the_permission_answer_is_refused() {
        // Coarse access says yes and there is no per-permission opinion, but
        // the batch left the id out: that is refused, not read as `None`.
        let checker = Some(FakeChecker {
            allowed: BTreeSet::from([41]),
            omitted: BTreeSet::from([41]),
            ..FakeChecker::default()
        });
        let h = harness(group_with_members(1, &[]), checker);
        let result = assign_project_to_group(
            RequireAuth(member_user()),
            State(h.state),
            Extension(metadata()),
            Path((1, 41)),
        )
        .await;
        let problem = result.err().expect("an unanswered project is refused");
        assert_eq!(problem.status_code, StatusCode::FORBIDDEN);
        assert_eq!(problem.body["title"], "Project Permission Denied");
    }

    #[tokio::test]
    async fn unknown_fields_in_the_body_are_a_400() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let h = harness(MockDatabase::new(DatabaseBackend::Postgres), None);
        for (method, uri, body) in [
            ("PATCH", "/project-groups/1", r#"{"slug":"renamed"}"#),
            ("POST", "/project-groups", r#"{"name":"CRM","id":7}"#),
        ] {
            let mut request = Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("valid request");
            request.extensions_mut().insert(member_user());
            request.extensions_mut().insert(metadata());
            let response = router(h.state.clone())
                .oneshot(request)
                .await
                .expect("router answers");
            let (status, problem) = json_of(response).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}: {problem}");
            assert_eq!(problem["title"], "Validation Error");
        }
    }

    #[test]
    fn openapi_documents_every_route_under_the_project_groups_tag() {
        let doc = ProjectGroupsApiDoc::openapi();
        let mut operations = 0;
        for (path, item) in &doc.paths.paths {
            assert!(path.starts_with("/project-groups"), "{path}");
            let methods = [&item.get, &item.post, &item.put, &item.patch, &item.delete];
            for operation in methods.into_iter().flatten() {
                operations += 1;
                assert_eq!(
                    operation.tags.as_deref(),
                    Some(&["Project Groups".to_string()][..])
                );
            }
        }
        assert_eq!(operations, 8);
        assert!(doc
            .components
            .as_ref()
            .is_some_and(|c| c.schemas.contains_key("ProjectGroupResponse")));
    }
}
