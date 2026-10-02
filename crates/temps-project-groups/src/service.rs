// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Persistence and rules for project groups (ADR-049).
//!
//! This layer knows nothing about callers: visibility and management rules
//! live in [`crate::visibility`] and are applied by the handlers, which hold
//! the auth context and the access checker.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use sea_orm::{
    sea_query::{Expr, LockType, OnConflict},
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, RelationTrait, Set, TransactionTrait,
};
use serde::Deserialize;
use temps_entities::{project_group_members, project_groups, projects};
use utoipa::ToSchema;

use crate::error::ProjectGroupError;

/// Longest name accepted, in characters. The column is `text`; the limit is
/// for the console, where a name is a heading and a breadcrumb crumb.
pub const MAX_NAME_CHARS: usize = 255;

/// Longest slug accepted, in bytes (slugs are ASCII). Matches team slugs.
pub const MAX_SLUG_LEN: usize = 64;

// ---------------------------------------------------------------------------
// Request DTOs
// ---------------------------------------------------------------------------

/// Unknown fields are refused so a typo (or an attempt to send `id`) is a
/// 400, not a silently ignored field.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProjectGroupRequest {
    pub name: String,
    /// Generated from `name` when omitted. Immutable afterwards.
    pub slug: Option<String>,
    pub description: Option<String>,
}

/// Only the fields present are changed. Absent and `null` both mean "not
/// provided"; an empty `description` clears it. Unknown fields are refused,
/// so sending `slug` (immutable) is a 400 rather than a silent no-op.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProjectGroupRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// A group plus the ids of its live members.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectGroupWithMembers {
    pub group: project_groups::Model,
    /// Members whose project is not soft-deleted, ascending by id.
    pub project_ids: Vec<i32>,
}

/// What [`ProjectGroupService::assign`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct AssignOutcome {
    pub group: ProjectGroupWithMembers,
    /// The group the project was in before, when it was in another one.
    pub previous_group_id: Option<i32>,
    /// False when the project was already in this group: nothing was
    /// written, and the caller must not record an audit entry.
    pub changed: bool,
}

/// Which fields an update actually changed, for the audit entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdatedFields {
    pub name: bool,
    pub description: bool,
}

impl UpdatedFields {
    pub fn any(&self) -> bool {
        self.name || self.description
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Derives a slug from a name: lowercase ASCII letters and digits, every
/// other run of characters collapsed into one `-`, trimmed, at most
/// [`MAX_SLUG_LEN`] bytes. Non-ASCII letters are dropped rather than
/// transliterated — a caller who wants `sao-paulo` from "São Paulo" gets
/// `s-o-paulo`, and can send the slug explicitly.
pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len().min(MAX_SLUG_LEN));
    let mut pending_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(c.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if slug.len() > MAX_SLUG_LEN {
        slug.truncate(MAX_SLUG_LEN);
        while slug.ends_with('-') {
            slug.pop();
        }
    }
    slug
}

pub fn validate_slug(slug: &str) -> Result<(), ProjectGroupError> {
    let well_formed = slug.len() <= MAX_SLUG_LEN && slug.split('-').all(is_slug_word);
    if well_formed {
        Ok(())
    } else {
        Err(ProjectGroupError::Validation {
            message: format!(
                "Slug '{slug}' must be 1-{MAX_SLUG_LEN} characters of a-z and 0-9, \
                 in words separated by single hyphens"
            ),
        })
    }
}

/// One hyphen-separated word of a slug. Empty words reject leading,
/// trailing and doubled hyphens, and the empty slug itself.
fn is_slug_word(word: &str) -> bool {
    let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    !word.is_empty() && word.bytes().all(allowed)
}

/// Returns the trimmed name, or a validation error.
pub fn validate_name(name: &str) -> Result<String, ProjectGroupError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ProjectGroupError::Validation {
            message: "Name cannot be empty".into(),
        });
    }
    let chars = trimmed.chars().count();
    if chars > MAX_NAME_CHARS {
        return Err(ProjectGroupError::Validation {
            message: format!("Name must be at most {MAX_NAME_CHARS} characters (got {chars})"),
        });
    }
    Ok(trimmed.to_string())
}

/// An empty or blank description means "none".
fn normalize_description(description: Option<String>) -> Option<String> {
    description.filter(|d| !d.trim().is_empty())
}

/// Attaches membership rows to their groups, keeping the group order and
/// the (already ascending) project order.
pub fn attach_members(
    groups: Vec<project_groups::Model>,
    members: Vec<project_group_members::Model>,
) -> Vec<ProjectGroupWithMembers> {
    let mut by_group: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    for member in members {
        by_group
            .entry(member.group_id)
            .or_default()
            .push(member.project_id);
    }
    groups
        .into_iter()
        .map(|group| {
            let mut project_ids = by_group.remove(&group.id).unwrap_or_default();
            project_ids.sort_unstable();
            ProjectGroupWithMembers { group, project_ids }
        })
        .collect()
}

/// The locks in `assign` keep both referenced rows alive until commit, so a
/// foreign-key violation here means the schema or the locking changed; it is
/// still reported as the missing row (404), never as a 500.
fn membership_write_error(
    source: sea_orm::DbErr,
    group_id: i32,
    project_id: i32,
) -> ProjectGroupError {
    match source.sql_err() {
        Some(sea_orm::SqlErr::ForeignKeyConstraintViolation(message))
            if message.contains("fk_project_group_members_project") =>
        {
            ProjectGroupError::ProjectNotFound { project_id }
        }
        Some(sea_orm::SqlErr::ForeignKeyConstraintViolation(_)) => {
            ProjectGroupError::NotFound { group_id }
        }
        _ => ProjectGroupError::Database {
            context: "writing the membership",
            source,
        },
    }
}

fn is_unique_violation(err: &sea_orm::DbErr) -> bool {
    matches!(err, sea_orm::DbErr::RecordNotInserted)
        || err
            .sql_err()
            .is_some_and(|e| matches!(e, sea_orm::SqlErr::UniqueConstraintViolation(_)))
}

/// Membership rows whose project is live. A soft-deleted project keeps its
/// row until the hard delete cascades it away, but must not be listed.
async fn live_members<C: ConnectionTrait>(
    conn: &C,
    group_id: Option<i32>,
) -> Result<Vec<project_group_members::Model>, ProjectGroupError> {
    let mut query = project_group_members::Entity::find()
        .join(
            sea_orm::JoinType::InnerJoin,
            project_group_members::Relation::Project.def(),
        )
        .filter(projects::Column::IsDeleted.eq(false))
        .filter(projects::Column::DeletedAt.is_null());
    if let Some(group_id) = group_id {
        query = query.filter(project_group_members::Column::GroupId.eq(group_id));
    }
    query
        .order_by_asc(project_group_members::Column::ProjectId)
        .all(conn)
        .await
        .map_err(ProjectGroupError::db("loading project group members"))
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

pub struct ProjectGroupService {
    db: Arc<DatabaseConnection>,
}

impl ProjectGroupService {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }

    /// Every group, ordered by name, with its live members.
    pub async fn list(&self) -> Result<Vec<ProjectGroupWithMembers>, ProjectGroupError> {
        let groups = project_groups::Entity::find()
            .order_by_asc(project_groups::Column::Name)
            .order_by_asc(project_groups::Column::Id)
            .all(self.db.as_ref())
            .await
            .map_err(ProjectGroupError::db("listing project groups"))?;
        let members = live_members(self.db.as_ref(), None).await?;
        Ok(attach_members(groups, members))
    }

    pub async fn get(&self, group_id: i32) -> Result<ProjectGroupWithMembers, ProjectGroupError> {
        let group = project_groups::Entity::find_by_id(group_id)
            .one(self.db.as_ref())
            .await
            .map_err(ProjectGroupError::db("loading a project group"))?
            .ok_or(ProjectGroupError::NotFound { group_id })?;
        self.with_members(group).await
    }

    pub async fn get_by_slug(
        &self,
        slug: &str,
    ) -> Result<ProjectGroupWithMembers, ProjectGroupError> {
        let group = project_groups::Entity::find()
            .filter(project_groups::Column::Slug.eq(slug))
            .one(self.db.as_ref())
            .await
            .map_err(ProjectGroupError::db("loading a project group by slug"))?
            .ok_or_else(|| ProjectGroupError::NotFoundBySlug {
                slug: slug.to_string(),
            })?;
        self.with_members(group).await
    }

    pub async fn create(
        &self,
        req: CreateProjectGroupRequest,
    ) -> Result<ProjectGroupWithMembers, ProjectGroupError> {
        let name = validate_name(&req.name)?;
        let slug = match req.slug {
            Some(slug) => slug.trim().to_string(),
            None => {
                let derived = slugify(&name);
                if derived.is_empty() {
                    return Err(ProjectGroupError::Validation {
                        message: format!(
                            "Cannot derive a slug from '{name}': it has no letters or digits \
                             a-z/0-9; send a slug explicitly"
                        ),
                    });
                }
                derived
            }
        };
        validate_slug(&slug)?;

        let now = Utc::now();
        let model = project_groups::ActiveModel {
            name: Set(name),
            slug: Set(slug.clone()),
            description: Set(normalize_description(req.description)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        match model.insert(self.db.as_ref()).await {
            Ok(group) => Ok(ProjectGroupWithMembers {
                group,
                project_ids: Vec::new(),
            }),
            Err(e) if is_unique_violation(&e) => Err(ProjectGroupError::SlugConflict { slug }),
            Err(source) => Err(ProjectGroupError::Database {
                context: "creating a project group",
                source,
            }),
        }
    }

    /// Applies the fields present in `req`. Writes nothing when no field
    /// would change, so a no-op PATCH neither bumps `updated_at` nor needs
    /// an audit entry.
    pub async fn update(
        &self,
        group_id: i32,
        req: UpdateProjectGroupRequest,
    ) -> Result<(ProjectGroupWithMembers, UpdatedFields), ProjectGroupError> {
        let name = req.name.as_deref().map(validate_name).transpose()?;

        let existing = project_groups::Entity::find_by_id(group_id)
            .one(self.db.as_ref())
            .await
            .map_err(ProjectGroupError::db("loading a project group"))?
            .ok_or(ProjectGroupError::NotFound { group_id })?;

        let mut changed = UpdatedFields::default();
        let mut active: project_groups::ActiveModel = existing.clone().into();
        if let Some(name) = name.filter(|n| *n != existing.name) {
            active.name = Set(name);
            changed.name = true;
        }
        if let Some(description) = req.description {
            let description = normalize_description(Some(description));
            if description != existing.description {
                active.description = Set(description);
                changed.description = true;
            }
        }

        if !changed.any() {
            return Ok((self.with_members(existing).await?, changed));
        }
        active.updated_at = Set(Utc::now());
        let group = active
            .update(self.db.as_ref())
            .await
            .map_err(ProjectGroupError::db("updating a project group"))?;
        Ok((self.with_members(group).await?, changed))
    }

    /// Deletes the group. Its membership rows cascade; the projects stay.
    pub async fn delete(&self, group_id: i32) -> Result<(), ProjectGroupError> {
        let result = project_groups::Entity::delete_by_id(group_id)
            .exec(self.db.as_ref())
            .await
            .map_err(ProjectGroupError::db("deleting a project group"))?;
        if result.rows_affected == 0 {
            return Err(ProjectGroupError::NotFound { group_id });
        }
        Ok(())
    }

    /// Puts `project_id` in `group_id`, moving it out of any other group in
    /// the same transaction (the membership key is the project).
    pub async fn assign(
        &self,
        group_id: i32,
        project_id: i32,
    ) -> Result<AssignOutcome, ProjectGroupError> {
        let txn = self
            .db
            .begin()
            .await
            .map_err(ProjectGroupError::db("starting the assign transaction"))?;

        // FOR KEY SHARE: a concurrent delete of this group waits for the
        // commit (then cascades the new row away) instead of failing our
        // insert on the foreign key. Not FOR SHARE: that conflicts with the
        // `updated_at` UPDATE below, so two assigns into the same group (or
        // two crossed moves) would each hold the share lock the other's
        // UPDATE waits on — a deadlock. KEY SHARE only conflicts with
        // deletes and key changes.
        let group = project_groups::Entity::find_by_id(group_id)
            .lock(LockType::KeyShare)
            .one(&txn)
            .await
            .map_err(ProjectGroupError::db("loading a project group"))?
            .ok_or(ProjectGroupError::NotFound { group_id })?;

        // The project row is the serialization point for moves of the same
        // project: the membership row below may not exist yet, and FOR UPDATE
        // locks nothing then, so two first-time assigns would both read "no
        // previous group". NO KEY UPDATE serializes them without blocking the
        // FOR KEY SHARE that foreign-key checks take on `projects`.
        let live_project: Option<i32> = projects::Entity::find_by_id(project_id)
            .select_only()
            .column(projects::Column::Id)
            .filter(projects::Column::IsDeleted.eq(false))
            .filter(projects::Column::DeletedAt.is_null())
            .lock(LockType::NoKeyUpdate)
            .into_tuple()
            .one(&txn)
            .await
            .map_err(ProjectGroupError::db("loading the project to assign"))?;
        if live_project.is_none() {
            return Err(ProjectGroupError::ProjectNotFound { project_id });
        }

        // With the project locked this read is stable: `previous_group_id`
        // in the audit entry and the `updated_at` bump below name the group
        // the project really left.
        let previous = project_group_members::Entity::find_by_id(project_id)
            .lock_exclusive()
            .one(&txn)
            .await
            .map_err(ProjectGroupError::db("loading the current membership"))?;
        let previous_group_id = previous.map(|m| m.group_id);

        if previous_group_id == Some(group_id) {
            txn.commit()
                .await
                .map_err(ProjectGroupError::db("committing the assign transaction"))?;
            return Ok(AssignOutcome {
                group: self.with_members(group).await?,
                previous_group_id: None,
                changed: false,
            });
        }

        let membership = project_group_members::ActiveModel {
            project_id: Set(project_id),
            group_id: Set(group_id),
        };
        project_group_members::Entity::insert(membership)
            .on_conflict(
                OnConflict::column(project_group_members::Column::ProjectId)
                    .update_column(project_group_members::Column::GroupId)
                    .to_owned(),
            )
            .exec_without_returning(&txn)
            .await
            .map_err(|source| membership_write_error(source, group_id, project_id))?;

        // Both groups' contents changed. One row per statement, in id
        // order, so crossed moves (A: G1→G2, B: G2→G1) take the two row
        // locks in the same order whatever plan Postgres picks.
        let mut touched: Vec<i32> = std::iter::once(group_id).chain(previous_group_id).collect();
        touched.sort_unstable();
        let now = Utc::now();
        for id in touched {
            project_groups::Entity::update_many()
                .col_expr(project_groups::Column::UpdatedAt, Expr::value(now))
                .filter(project_groups::Column::Id.eq(id))
                .exec(&txn)
                .await
                .map_err(ProjectGroupError::db("touching the project groups"))?;
        }

        txn.commit()
            .await
            .map_err(ProjectGroupError::db("committing the assign transaction"))?;

        Ok(AssignOutcome {
            group: self.get(group_id).await?,
            previous_group_id,
            changed: true,
        })
    }

    /// Takes `project_id` out of `group_id`. The project stays, ungrouped.
    pub async fn unassign(&self, group_id: i32, project_id: i32) -> Result<(), ProjectGroupError> {
        let txn = self
            .db
            .begin()
            .await
            .map_err(ProjectGroupError::db("starting the unassign transaction"))?;
        let result = project_group_members::Entity::delete_many()
            .filter(project_group_members::Column::ProjectId.eq(project_id))
            .filter(project_group_members::Column::GroupId.eq(group_id))
            .exec(&txn)
            .await
            .map_err(ProjectGroupError::db("removing the membership"))?;
        if result.rows_affected == 0 {
            return Err(ProjectGroupError::NotAMember {
                group_id,
                project_id,
            });
        }
        project_groups::Entity::update_many()
            .col_expr(project_groups::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(project_groups::Column::Id.eq(group_id))
            .exec(&txn)
            .await
            .map_err(ProjectGroupError::db("touching the project group"))?;
        txn.commit()
            .await
            .map_err(ProjectGroupError::db("committing the unassign transaction"))?;
        Ok(())
    }

    async fn with_members(
        &self,
        group: project_groups::Model,
    ) -> Result<ProjectGroupWithMembers, ProjectGroupError> {
        let members = live_members(self.db.as_ref(), Some(group.id)).await?;
        Ok(ProjectGroupWithMembers {
            group,
            project_ids: members.into_iter().map(|m| m.project_id).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DatabaseBackend, DbErr, MockDatabase, MockExecResult, Value};

    fn group_row(id: i32, name: &str, slug: &str) -> project_groups::Model {
        let now = Utc::now();
        project_groups::Model {
            id,
            name: name.into(),
            slug: slug.into(),
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

    fn exec(rows_affected: u64) -> MockExecResult {
        MockExecResult {
            last_insert_id: 0,
            rows_affected,
        }
    }

    fn id_row(id: i32) -> BTreeMap<&'static str, Value> {
        BTreeMap::from([("id", Value::from(id))])
    }

    fn service(db: MockDatabase) -> (ProjectGroupService, Arc<DatabaseConnection>) {
        let conn = Arc::new(db.into_connection());
        (ProjectGroupService::new(conn.clone()), conn)
    }

    /// Every SQL statement the mock saw, in order.
    fn statements(svc: ProjectGroupService, conn: Arc<DatabaseConnection>) -> Vec<String> {
        drop(svc);
        let conn = Arc::try_unwrap(conn).expect("service dropped, connection unshared");
        conn.into_transaction_log()
            .into_iter()
            .flat_map(|txn| {
                txn.statements()
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn slugify_lowercases_and_collapses_separators() {
        assert_eq!(slugify("CRM Interno"), "crm-interno");
        assert_eq!(slugify("  --Back__end  v2!! "), "back-end-v2");
        assert_eq!(slugify("São Paulo"), "s-o-paulo");
        assert_eq!(slugify("!!!"), "");
        let long = slugify(&"a-".repeat(40));
        assert!(long.len() <= MAX_SLUG_LEN && !long.ends_with('-'));
    }

    #[test]
    fn validate_slug_accepts_only_hyphenated_lowercase_words() {
        assert!(validate_slug("crm-2").is_ok());
        for bad in [
            "",
            "-crm",
            "crm-",
            "crm--x",
            "CRM",
            "crm x",
            &"a".repeat(65),
        ] {
            assert!(validate_slug(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn attach_members_groups_rows_and_keeps_ascending_ids() {
        let groups = vec![group_row(1, "A", "a"), group_row(2, "B", "b")];
        let attached = attach_members(groups, vec![member(9, 1), member(3, 1), member(4, 3)]);
        assert_eq!(attached[0].project_ids, vec![3, 9]);
        assert!(attached[1].project_ids.is_empty());
    }

    #[tokio::test]
    async fn list_returns_groups_with_their_live_members() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![
                group_row(1, "Back", "back"),
                group_row(2, "Ops", "ops"),
            ]])
            .append_query_results([vec![member(4, 1), member(7, 1)]]);
        let (svc, _conn) = service(db);
        let groups = svc.list().await.expect("list succeeds");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].project_ids, vec![4, 7]);
        assert!(groups[1].project_ids.is_empty());
    }

    #[tokio::test]
    async fn members_query_excludes_soft_deleted_projects() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "Back", "back")]])
            .append_query_results([Vec::<project_group_members::Model>::new()]);
        let (svc, conn) = service(db);
        svc.get(1).await.expect("get succeeds");
        let sql = statements(svc, conn).join("\n");
        assert!(sql.contains(r#""projects"."is_deleted" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""projects"."deleted_at" IS NULL"#), "{sql}");
    }

    #[tokio::test]
    async fn get_returns_not_found_for_missing_id() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([Vec::<project_groups::Model>::new()]);
        let (svc, _conn) = service(db);
        let err = svc.get(99).await.unwrap_err();
        assert!(matches!(err, ProjectGroupError::NotFound { group_id: 99 }));
    }

    #[tokio::test]
    async fn get_by_slug_returns_not_found_for_missing_slug() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([Vec::<project_groups::Model>::new()]);
        let (svc, _conn) = service(db);
        let err = svc.get_by_slug("nope").await.unwrap_err();
        assert!(matches!(err, ProjectGroupError::NotFoundBySlug { ref slug } if slug == "nope"));
    }

    #[tokio::test]
    async fn create_rejects_blank_name_without_touching_the_database() {
        let (svc, _conn) = service(MockDatabase::new(DatabaseBackend::Postgres));
        let err = svc
            .create(CreateProjectGroupRequest {
                name: "   ".into(),
                slug: None,
                description: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ProjectGroupError::Validation { .. }));
    }

    #[tokio::test]
    async fn create_rejects_an_invalid_explicit_slug() {
        let (svc, _conn) = service(MockDatabase::new(DatabaseBackend::Postgres));
        let err = svc
            .create(CreateProjectGroupRequest {
                name: "CRM".into(),
                slug: Some("Not Valid".into()),
                description: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ProjectGroupError::Validation { .. }));
    }

    #[tokio::test]
    async fn create_rejects_a_name_with_nothing_to_slugify() {
        let (svc, _conn) = service(MockDatabase::new(DatabaseBackend::Postgres));
        let err = svc
            .create(CreateProjectGroupRequest {
                name: "!!!".into(),
                slug: None,
                description: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ProjectGroupError::Validation { .. }));
    }

    #[tokio::test]
    async fn create_derives_the_slug_and_drops_an_empty_description() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(3, "CRM Interno", "crm-interno")]]);
        let (svc, conn) = service(db);
        let created = svc
            .create(CreateProjectGroupRequest {
                name: "  CRM Interno ".into(),
                slug: None,
                description: Some("  ".into()),
            })
            .await
            .expect("create succeeds");
        assert!(created.project_ids.is_empty());
        let sql = statements(svc, conn).join("\n");
        assert!(sql.contains("'crm-interno'"), "{sql}");
        assert!(sql.contains("'CRM Interno'"), "name is trimmed: {sql}");
        assert!(
            sql.contains("NULL"),
            "blank description stored as NULL: {sql}"
        );
    }

    #[tokio::test]
    async fn create_maps_a_unique_violation_to_slug_conflict() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_errors([DbErr::RecordNotInserted]);
        let (svc, _conn) = service(db);
        let err = svc
            .create(CreateProjectGroupRequest {
                name: "CRM".into(),
                slug: Some("crm".into()),
                description: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ProjectGroupError::SlugConflict { ref slug } if slug == "crm"));
    }

    #[tokio::test]
    async fn update_without_changes_writes_nothing() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "CRM", "crm")]])
            .append_query_results([Vec::<project_group_members::Model>::new()]);
        let (svc, conn) = service(db);
        let (_, changed) = svc
            .update(
                1,
                UpdateProjectGroupRequest {
                    name: Some(" CRM ".into()),
                    description: None,
                },
            )
            .await
            .expect("update succeeds");
        assert!(!changed.any());
        let sql = statements(svc, conn);
        assert!(sql.iter().all(|s| s.starts_with("SELECT")), "{sql:?}");
    }

    #[tokio::test]
    async fn update_with_empty_description_clears_it() {
        let mut described = group_row(1, "CRM", "crm");
        described.description = Some("old".into());
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![described]])
            .append_query_results([vec![group_row(1, "CRM", "crm")]])
            .append_query_results([Vec::<project_group_members::Model>::new()]);
        let (svc, conn) = service(db);
        let (updated, changed) = svc
            .update(
                1,
                UpdateProjectGroupRequest {
                    name: None,
                    description: Some(String::new()),
                },
            )
            .await
            .expect("update succeeds");
        assert!(changed.description && !changed.name);
        assert_eq!(updated.group.description, None);
        let sql = statements(svc, conn).join("\n");
        assert!(sql.contains(r#""description" = NULL"#), "{sql}");
    }

    #[tokio::test]
    async fn update_rejects_a_blank_name_before_loading() {
        let (svc, _conn) = service(MockDatabase::new(DatabaseBackend::Postgres));
        let err = svc
            .update(
                1,
                UpdateProjectGroupRequest {
                    name: Some(String::new()),
                    description: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ProjectGroupError::Validation { .. }));
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_nothing_was_deleted() {
        let db = MockDatabase::new(DatabaseBackend::Postgres).append_exec_results([exec(0)]);
        let (svc, _conn) = service(db);
        let err = svc.delete(5).await.unwrap_err();
        assert!(matches!(err, ProjectGroupError::NotFound { group_id: 5 }));
    }

    #[tokio::test]
    async fn assign_moves_the_project_and_reports_the_previous_group() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(2, "Ops", "ops")]])
            .append_query_results([vec![id_row(41)]])
            .append_query_results([vec![member(41, 1)]])
            .append_exec_results([exec(1), exec(1), exec(1)])
            // `get` after the commit: the group, then its members.
            .append_query_results([vec![group_row(2, "Ops", "ops")]])
            .append_query_results([vec![member(41, 2)]]);
        let (svc, conn) = service(db);
        let outcome = svc.assign(2, 41).await.expect("assign succeeds");
        assert!(outcome.changed);
        assert_eq!(outcome.previous_group_id, Some(1));
        assert_eq!(outcome.group.project_ids, vec![41]);
        let sql = statements(svc, conn).join("\n");
        assert!(
            sql.contains("ON CONFLICT (\"project_id\") DO UPDATE"),
            "{sql}"
        );
        assert!(
            sql.contains("FOR KEY SHARE"),
            "group is key-share locked: {sql}"
        );
        assert!(!sql.contains(" FOR SHARE"), "FOR SHARE deadlocks: {sql}");
        assert!(
            sql.contains("FOR NO KEY UPDATE"),
            "project is locked: {sql}"
        );
        assert!(sql.contains("FOR UPDATE"), "{sql}");
    }

    #[tokio::test]
    async fn assign_to_the_current_group_is_a_no_op() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "CRM", "crm")]])
            .append_query_results([vec![id_row(41)]])
            .append_query_results([vec![member(41, 1)]])
            .append_query_results([vec![member(41, 1)]]);
        let (svc, conn) = service(db);
        let outcome = svc.assign(1, 41).await.expect("assign succeeds");
        assert!(!outcome.changed);
        assert_eq!(outcome.previous_group_id, None);
        let sql = statements(svc, conn).join("\n");
        assert!(!sql.contains("INSERT"), "{sql}");
        assert!(!sql.contains("UPDATE \"project_groups\""), "{sql}");
    }

    #[tokio::test]
    async fn assign_rejects_a_missing_or_soft_deleted_project() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([vec![group_row(1, "CRM", "crm")]])
            .append_query_results([Vec::<BTreeMap<&'static str, Value>>::new()]);
        let (svc, _conn) = service(db);
        let err = svc.assign(1, 41).await.unwrap_err();
        assert!(matches!(
            err,
            ProjectGroupError::ProjectNotFound { project_id: 41 }
        ));
    }

    #[tokio::test]
    async fn assign_rejects_a_missing_group() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([Vec::<project_groups::Model>::new()]);
        let (svc, _conn) = service(db);
        let err = svc.assign(8, 41).await.unwrap_err();
        assert!(matches!(err, ProjectGroupError::NotFound { group_id: 8 }));
    }

    #[tokio::test]
    async fn unassign_reports_a_project_that_is_not_a_member() {
        let db = MockDatabase::new(DatabaseBackend::Postgres).append_exec_results([exec(0)]);
        let (svc, _conn) = service(db);
        let err = svc.unassign(1, 41).await.unwrap_err();
        assert!(matches!(
            err,
            ProjectGroupError::NotAMember {
                group_id: 1,
                project_id: 41
            }
        ));
    }

    #[tokio::test]
    async fn unassign_removes_the_row_and_touches_the_group() {
        let db =
            MockDatabase::new(DatabaseBackend::Postgres).append_exec_results([exec(1), exec(1)]);
        let (svc, conn) = service(db);
        svc.unassign(1, 41).await.expect("unassign succeeds");
        let sql = statements(svc, conn).join("\n");
        assert!(
            sql.contains("DELETE FROM \"project_group_members\""),
            "{sql}"
        );
        assert!(sql.contains("UPDATE \"project_groups\""), "{sql}");
    }
}
