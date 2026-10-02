// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `service_populate_runs`: one row per "populate a database of an existing
//! PostgreSQL service from an external PostgreSQL" operation.
//!
//! `restore_runs` cannot hold these: its `source_backup_id` is a mandatory
//! foreign key to `backups`, and a populate has no backup — its source is an
//! external database URL. The URL itself is never stored, only a copy with
//! the credentials masked.
//!
//! The partial unique index on `(service_id, database_name)` over running
//! rows is the authoritative lock: two concurrent copies into the same
//! database would interleave their DDL.
//!
//! Rolling back to an image older than this migration (e.g.
//! `v0.1.0-nextlw.8`) is NOT transparent: that image's migrator finds this
//! migration recorded in `seaql_migrations` without knowing it, and refuses
//! to start. Before starting the older image, run:
//!
//! ```sql
//! DELETE FROM seaql_migrations WHERE version = 'm20261002_000002_add_disconnect_clients_to_service_populate_runs';
//! DELETE FROM seaql_migrations WHERE version = 'm20261002_000001_create_service_populate_runs';
//! ```
//!
//! The table itself can stay: no older code references it, and keeping it
//! keeps the run history if the newer image comes back.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS service_populate_runs (
                id SERIAL PRIMARY KEY,
                service_id INTEGER NOT NULL
                    REFERENCES external_services (id) ON DELETE CASCADE,
                database_name TEXT NOT NULL,
                source_url_masked TEXT NOT NULL,
                replace_existing BOOLEAN NOT NULL DEFAULT FALSE,
                status TEXT NOT NULL,
                client_image TEXT NOT NULL,
                error_message TEXT NULL,
                database_size_bytes BIGINT NULL,
                started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                finished_at TIMESTAMPTZ NULL,
                created_by INTEGER NULL REFERENCES users (id) ON DELETE SET NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .await?;

        // Listing a service's runs, newest first.
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_service_populate_runs_service_created \
             ON service_populate_runs (service_id, created_at DESC)",
        )
        .await?;

        // At most one running copy per destination database.
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_service_populate_runs_one_running \
             ON service_populate_runs (service_id, database_name) \
             WHERE status = 'running'",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS service_populate_runs")
            .await?;
        Ok(())
    }
}
