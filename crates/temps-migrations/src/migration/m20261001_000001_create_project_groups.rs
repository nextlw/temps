// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Project groups: an entity above `projects` (ADR-049; the UI calls it
//! "Project" and today's project "Service", see ADR-048).
//!
//! Membership lives in its own table instead of a `projects.group_id`
//! column: a new column would change `projects::Model`, and every struct
//! literal of that model across the workspace (and every new fixture merged
//! from upstream) would have to list it. Keeping the feature in two new
//! tables leaves `projects` untouched, which is also what makes rolling back
//! to an older image safe — nothing existing references these tables.
//!
//! - `project_groups` — name, unique (immutable) slug, optional description.
//! - `project_group_members` — `project_id` is the primary key, so a project
//!   belongs to at most one group and moving it is an upsert on that key.
//!   Both foreign keys cascade: deleting a group only ungroups its projects,
//!   and hard-deleting a project drops its membership row.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProjectGroups::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ProjectGroups::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ProjectGroups::Name).text().not_null())
                    .col(
                        ColumnDef::new(ProjectGroups::Slug)
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(ProjectGroups::Description).text().null())
                    .col(
                        ColumnDef::new(ProjectGroups::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(ProjectGroups::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ProjectGroupMembers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ProjectGroupMembers::ProjectId)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ProjectGroupMembers::GroupId)
                            .integer()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_project_group_members_project")
                            .from(ProjectGroupMembers::Table, ProjectGroupMembers::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_project_group_members_group")
                            .from(ProjectGroupMembers::Table, ProjectGroupMembers::GroupId)
                            .to(ProjectGroups::Table, ProjectGroups::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Every group read loads its members by group_id, and the cascade
        // from `project_groups` scans the same column.
        manager
            .create_index(
                Index::create()
                    .name("idx_project_group_members_group")
                    .table(ProjectGroupMembers::Table)
                    .col(ProjectGroupMembers::GroupId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Members first: it holds the foreign key to `project_groups`.
        manager
            .drop_table(
                Table::drop()
                    .table(ProjectGroupMembers::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(ProjectGroups::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum ProjectGroups {
    Table,
    Id,
    Name,
    Slug,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ProjectGroupMembers {
    Table,
    ProjectId,
    GroupId,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}
