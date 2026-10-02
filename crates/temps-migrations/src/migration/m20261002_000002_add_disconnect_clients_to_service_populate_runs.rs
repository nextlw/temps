// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `service_populate_runs.disconnect_clients`: whether a `replace` run was
//! allowed to terminate the sessions of the destination database. Without
//! it, a `replace` is refused while the destination has sessions.
//!
//! For a rollback to an image older than
//! `m20261002_000001_create_service_populate_runs`, see the SQL in that
//! migration's documentation.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE service_populate_runs \
                 ADD COLUMN IF NOT EXISTS disconnect_clients BOOLEAN NOT NULL DEFAULT FALSE",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE service_populate_runs DROP COLUMN IF EXISTS disconnect_clients",
            )
            .await?;
        Ok(())
    }
}
