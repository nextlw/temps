// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Accept digest-pinned daemon images from any GHCR owner, not only gotempsh.
//!
//! A release embeds the daemon images of the repository that cut it
//! (`ghcr.io/<owner>/temps-sandbox-*`, see `temps_core::release_manifest`), so
//! a fork's release (ghcr.io/nextlw/*) was rejected by the constraint that
//! m20260914 limited to ghcr.io/gotempsh. The application layer
//! (`is_managed_application_workspace_image`) still restricts the owner to
//! gotempsh or the build's own namespace; this check only keeps the shape.

use sea_orm_migration::prelude::*;

use super::m20260914_000001_managed_daemon_digest_images as previous;

#[derive(DeriveMigrationName)]
pub struct Migration;

const DIGEST_PREDICATE: &str =
    "image ~ '^ghcr[.]io/[a-z0-9-]+/temps-sandbox-(nodejs|python|all)@sha256:[0-9a-f]{64}$'";

fn up_sql() -> Result<String, DbErr> {
    let previous_sql = previous::up_sql()?;
    if !previous_sql.contains(previous::DIGEST_PREDICATE) {
        return Err(DbErr::Custom(
            "previous managed daemon digest constraint has an unexpected shape".into(),
        ));
    }
    Ok(previous_sql.replace(previous::DIGEST_PREDICATE, DIGEST_PREDICATE))
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(&up_sql()?)
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // PostgreSQL rejects the downgrade while a workspace still pins a
        // non-gotempsh digest, which is the safe outcome.
        manager
            .get_connection()
            .execute_unprepared(&previous::up_sql()?)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_digest_constraint_keeps_history_and_only_widens_the_owner() {
        let sql = up_sql().expect("valid previous constraint");
        for tag in ["node:0.1.0", "nodejs:0.2.0", "python:0.3.0", "all:0.3.4"] {
            assert!(
                sql.contains(tag),
                "historical tag {tag} must remain allowed"
            );
        }
        assert!(sql.contains(DIGEST_PREDICATE));
        assert!(!sql.contains(previous::DIGEST_PREDICATE));
        assert_eq!(
            DIGEST_PREDICATE.replace("[a-z0-9-]+", "gotempsh"),
            previous::DIGEST_PREDICATE
        );
        assert!(!DIGEST_PREDICATE.contains("temps-sandbox-bun"));
    }
}
