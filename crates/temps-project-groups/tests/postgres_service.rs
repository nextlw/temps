// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The service against a real PostgreSQL, for what MockDatabase cannot
//! show: the unique violation Postgres actually raises (23505) becoming a
//! 409, and the locking, upsert and join SQL running as written.
//!
//! Skips when Docker is unavailable, like `temps-migrations`' suite.

use std::sync::Arc;
use std::time::Duration;

use sea_orm::{ConnectionTrait, Database, DatabaseConnection, TransactionTrait};
use temps_migrations::{Migrator, MigratorTrait};
use temps_project_groups::{CreateProjectGroupRequest, ProjectGroupError, ProjectGroupService};
use testcontainers::{core::WaitFor, runners::AsyncRunner, GenericImage, ImageExt};

async fn connect(db_url: &str) -> anyhow::Result<DatabaseConnection> {
    let mut last_err = None;
    for _ in 0..15 {
        match Database::connect(db_url).await {
            Ok(db) => return Ok(db),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    Err(anyhow::anyhow!("could not connect to {db_url}: {last_err:?}"))
}

fn request(name: &str, slug: Option<&str>) -> CreateProjectGroupRequest {
    CreateProjectGroupRequest {
        name: name.into(),
        slug: slug.map(str::to_string),
        description: None,
    }
}

#[tokio::test]
async fn service_against_postgres() -> anyhow::Result<()> {
    let container = match GenericImage::new("timescale/timescaledb-ha", "pg18")
        .with_wait_for(WaitFor::message_on_stderr("database system is ready to accept connections"))
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_HOST_AUTH_METHOD", "trust")
        .with_cmd(vec![
            "postgres",
            "-c",
            "timescaledb.max_background_workers=0",
        ])
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
    {
        Ok(container) => container,
        Err(error) => {
            eprintln!("⏭️  Skipping service_against_postgres: Docker unavailable ({error})");
            return Ok(());
        }
    };
    let port = container.get_host_port_ipv4(5432).await?;
    let db_url = format!("postgresql://postgres:postgres@localhost:{port}/postgres");
    let db = connect(&db_url).await?;
    Migrator::up(&db, None).await?;
    db.execute_unprepared(
        "INSERT INTO projects \
         (id, name, repo_name, repo_owner, directory, main_branch, preset, created_at, updated_at, slug) \
         VALUES \
         (41, 'Back', '', '', '.', 'main', 'dockerfile', now(), now(), 'back'), \
         (42, 'Front', '', '', '.', 'main', 'dockerfile', now(), now(), 'front')",
    )
    .await?;
    let svc = Arc::new(ProjectGroupService::new(Arc::new(db)));

    // 409 from the real unique constraint, generated and explicit slugs.
    let crm = svc.create(request("CRM", None)).await?;
    let ops = svc.create(request("Ops", None)).await?;
    for duplicate in [request("CRM", None), request("Other", Some("crm"))] {
        let err = svc.create(duplicate).await.unwrap_err();
        assert!(
            matches!(err, ProjectGroupError::SlugConflict { ref slug } if slug == "crm"),
            "{err:?}"
        );
    }

    // First assign, idempotent re-assign, move, unassign.
    let first = svc.assign(crm.group.id, 41).await?;
    assert!(first.changed && first.previous_group_id.is_none());
    let again = svc.assign(crm.group.id, 41).await?;
    assert!(!again.changed);
    let moved = svc.assign(ops.group.id, 41).await?;
    assert_eq!(moved.previous_group_id, Some(crm.group.id));
    assert_eq!(moved.group.project_ids, vec![41]);
    assert!(svc.get(crm.group.id).await?.project_ids.is_empty());

    // Assigns of one project serialize on its row even before any
    // membership row exists: while another transaction holds the project
    // row, assign waits. (The membership insert alone only takes FOR KEY
    // SHARE on `projects`, which would not wait.)
    let other = connect(&db_url).await?;
    let blocker = other.begin().await?;
    blocker
        .execute_unprepared("SELECT id FROM projects WHERE id = 42 FOR NO KEY UPDATE")
        .await?;
    let pending = tokio::spawn({
        let svc = svc.clone();
        let group_id = crm.group.id;
        async move { svc.assign(group_id, 42).await }
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!pending.is_finished(), "assign must wait for the project row");
    blocker.commit().await?;
    let assigned = pending.await??;
    assert!(assigned.changed && assigned.previous_group_id.is_none());

    // Missing rows are 404s, not 500s.
    let err = svc.assign(crm.group.id, 999).await.unwrap_err();
    assert!(matches!(err, ProjectGroupError::ProjectNotFound { project_id: 999 }));
    let err = svc.assign(9999, 41).await.unwrap_err();
    assert!(matches!(err, ProjectGroupError::NotFound { group_id: 9999 }));

    Ok(())
}
