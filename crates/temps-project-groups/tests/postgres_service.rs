// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The service against a real PostgreSQL, for what MockDatabase cannot
//! show: the unique violation Postgres actually raises (23505) becoming a
//! 409, the foreign-key fallback, and the locking under real concurrency.
//!
//! Each test skips when Docker is unavailable, like `temps-migrations`'
//! suite.

use std::sync::Arc;
use std::time::Duration;

use sea_orm::{ConnectionTrait, Database, DatabaseConnection, TransactionTrait};
use temps_migrations::{Migrator, MigratorTrait};
use temps_project_groups::{CreateProjectGroupRequest, ProjectGroupError, ProjectGroupService};
use testcontainers::{core::WaitFor, runners::AsyncRunner, ContainerAsync, GenericImage, ImageExt};

/// A migrated database with projects 41, 42 and 43, or `None` without
/// Docker. The container must outlive the test, so it is returned too.
async fn database(test: &str) -> anyhow::Result<Option<(ContainerAsync<GenericImage>, String)>> {
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
            eprintln!("⏭️  Skipping {test}: Docker unavailable ({error})");
            return Ok(None);
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
         (42, 'Front', '', '', '.', 'main', 'dockerfile', now(), now(), 'front'), \
         (43, 'Worker', '', '', '.', 'main', 'dockerfile', now(), now(), 'worker')",
    )
    .await?;
    Ok(Some((container, db_url)))
}

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
    let Some((_container, db_url)) = database("service_against_postgres").await? else {
        return Ok(());
    };
    let svc = Arc::new(ProjectGroupService::new(Arc::new(connect(&db_url).await?)));

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
    svc.unassign(ops.group.id, 41).await?;

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
    assert!(
        !pending.is_finished(),
        "assign must wait for the project row"
    );
    blocker.commit().await?;
    let assigned = pending.await??;
    assert!(assigned.changed && assigned.previous_group_id.is_none());

    // A concurrent delete of the target group waits for the assign to
    // commit, then cascades its membership away.
    let doomed = svc.create(request("Doomed", None)).await?;
    let holder = other.begin().await?;
    holder
        .execute_unprepared(&format!(
            "SELECT id FROM project_groups WHERE id = {} FOR KEY SHARE",
            doomed.group.id
        ))
        .await?;
    let delete = tokio::spawn({
        let svc = svc.clone();
        let group_id = doomed.group.id;
        async move { svc.delete(group_id).await }
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !delete.is_finished(),
        "a group delete waits for key-share holders"
    );
    holder.commit().await?;
    delete.await??;

    // Missing rows are 404s, not 500s.
    let err = svc.assign(crm.group.id, 999).await.unwrap_err();
    assert!(matches!(err, ProjectGroupError::ProjectNotFound { project_id: 999 }));
    let err = svc.assign(9999, 41).await.unwrap_err();
    assert!(matches!(err, ProjectGroupError::NotFound { group_id: 9999 }));

    Ok(())
}

/// Two assigns into the same group, and two crossed moves, run at the same
/// time without a deadlock. With the group read `FOR SHARE` each
/// transaction holds the share lock the other's `updated_at` UPDATE waits
/// on, and Postgres aborts one with 40P01.
#[tokio::test]
async fn concurrent_assigns_do_not_deadlock() -> anyhow::Result<()> {
    let Some((_container, db_url)) = database("concurrent_assigns_do_not_deadlock").await? else {
        return Ok(());
    };
    let db = Arc::new(connect(&db_url).await?);
    let svc = ProjectGroupService::new(db.clone());
    let g1 = svc.create(request("G1", None)).await?.group.id;
    let g2 = svc.create(request("G2", None)).await?.group.id;

    for round in 0..15 {
        db.execute_unprepared("DELETE FROM project_group_members")
            .await?;

        // Different services, same group.
        let (a, b) = tokio::join!(svc.assign(g1, 41), svc.assign(g1, 42));
        a.map_err(|e| anyhow::anyhow!("round {round}, same group, 41: {e}"))?;
        b.map_err(|e| anyhow::anyhow!("round {round}, same group, 42: {e}"))?;

        // Crossed moves: 41 G1→G2 while 42 G2→G1.
        svc.assign(g2, 42).await?;
        let (a, b) = tokio::join!(svc.assign(g2, 41), svc.assign(g1, 42));
        let a = a.map_err(|e| anyhow::anyhow!("round {round}, crossed, 41: {e}"))?;
        let b = b.map_err(|e| anyhow::anyhow!("round {round}, crossed, 42: {e}"))?;
        assert_eq!(
            (a.previous_group_id, b.previous_group_id),
            (Some(g1), Some(g2))
        );
    }
    Ok(())
}

/// The locks keep both referenced rows alive, so the foreign-key fallback
/// is forced with a trigger that deletes the row inside the assign's own
/// transaction, right before the membership insert: the real Postgres
/// violation must still come back as a 404 for the row that vanished.
#[tokio::test]
async fn foreign_key_violation_on_assign_is_a_not_found() -> anyhow::Result<()> {
    let test = "foreign_key_violation_on_assign_is_a_not_found";
    let Some((_container, db_url)) = database(test).await? else {
        return Ok(());
    };
    let db = Arc::new(connect(&db_url).await?);
    let svc = ProjectGroupService::new(db.clone());
    let group = svc.create(request("Vanishing", None)).await?.group.id;

    db.execute_unprepared(
        "CREATE FUNCTION pg_test_drop_group() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN DELETE FROM project_groups WHERE id = NEW.group_id; RETURN NEW; END $$; \
         CREATE FUNCTION pg_test_drop_project() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN DELETE FROM projects WHERE id = NEW.project_id; RETURN NEW; END $$",
    )
    .await?;

    db.execute_unprepared(
        "CREATE TRIGGER pg_test_vanish BEFORE INSERT ON project_group_members \
         FOR EACH ROW EXECUTE FUNCTION pg_test_drop_group()",
    )
    .await?;
    let err = svc.assign(group, 41).await.unwrap_err();
    assert!(
        matches!(err, ProjectGroupError::NotFound { group_id } if group_id == group),
        "{err:?}"
    );

    db.execute_unprepared(
        "DROP TRIGGER pg_test_vanish ON project_group_members; \
         CREATE TRIGGER pg_test_vanish BEFORE INSERT ON project_group_members \
         FOR EACH ROW EXECUTE FUNCTION pg_test_drop_project()",
    )
    .await?;
    let err = svc.assign(group, 43).await.unwrap_err();
    assert!(
        matches!(err, ProjectGroupError::ProjectNotFound { project_id: 43 }),
        "{err:?}"
    );

    // Rolled back with the failed transaction: both rows are still there.
    assert_eq!(svc.get(group).await?.group.id, group);
    Ok(())
}
