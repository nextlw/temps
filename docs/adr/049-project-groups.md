# ADR-049: Project groups — a `project_group` entity above `project`

**Status:** Accepted
**Date:** 2026-10-01
**Builds on:** ADR-048 (UI terminology — Projects contain Services; the code keeps `project`)
**Scope:** new backend crate `temps-project-groups`, an additive migration, new
entities, a new REST surface under `/project-groups`, and the web console. The
existing `project` API, CLI, SDK, plugin events and AI tool contracts do not change.

## Context

ADR-048 fixed the vocabulary: on screen today's `project` reads **Service**, and
the entity that groups services reads **Project**. It left that second entity
"added later". This ADR defines it.

Users deploy one system as several deployable units that ship together — the
backend and the frontend of one CRM, say. Today each unit is an unrelated
`project`: nothing says that two of them belong to the same system, so the
console cannot list them together, and later work (variables shared by the
system, one preview environment per pull request covering all its units) has no
entity to hang off.

Two existing concepts look like candidates and are not:

- **Docker Compose.** Compose describes several containers that are built and
  deployed *as one unit* of a single `project`. The point of the new entity is
  the opposite: each service keeps its own repository, its own deployments, and
  is deployed and rolled back on its own. Compose services also already carry
  the word "service" in code (`crates/temps-presets/src/docker_compose.rs`,
  listed in ADR-048).
- **The AI workspace (`ai_applications`).** It is private to the user who
  created it — every query filters on `created_by`
  (`crates/temps-ai-chat/src/applications.rs`, for example the
  `Column::CreatedBy.eq(user_id)` filters at lines 1962 and 2067) — and it is
  built around AI sessions. A system shared by a team, visible to everyone who
  can see its services, cannot live there. ADR-048 rule 6 already keeps it a
  separate entity; this ADR keeps that.

## Decision

Add `project_group` (UI: **Project**) as a new entity in its own crate. A
`project_group` has a name, a unique slug, an optional description and zero or
more member `project`s (UI: **Services**). Code, SQL, API and audit names use
`project_group`; the UI words come from the `terms` namespace as ADR-048 says.

### DF2-1. Data model: a membership table, not a column on `projects`

Two new tables, created by one additive migration:

```
project_groups
  id           serial PK
  name         text        not null
  slug         text        not null UNIQUE
  description  text        null
  created_at   timestamptz not null
  updated_at   timestamptz not null

project_group_members
  project_id   integer PK  -> projects(id)       ON DELETE CASCADE
  group_id     integer     -> project_groups(id) ON DELETE CASCADE   (indexed)
```

- **`project_id` is the primary key** of the membership table: a service
  belongs to at most one Project. Moving a service is an upsert on that key.
- **Cascades both ways.** Deleting a group removes only its membership rows —
  the services stay and become ungrouped. Hard-deleting a service removes its
  membership row. Soft-deleted services (`projects.deleted_at`) are not listed
  as members in responses.
- **A new crate**, `crates/temps-project-groups`, modelled on
  `crates/temps-teams`: service, handlers, plugin, `utoipa` documentation. New
  entities live in `crates/temps-entities/src/project_groups.rs` and
  `project_group_members.rs`, with relations only on the new side.

**Why not `projects.group_id`?** It is the obvious schema, and it is the wrong
one for this fork. A new column on `projects` changes `projects::Model`, and
every struct literal of that model must then list the field. A search of the
tree finds `projects::Model {` literals in 25 files across 14 crates; each new
fixture upstream writes would fail to compile after every merge of
`gotempsh/temps`. A separate table leaves `crates/temps-entities/src/projects.rs`
untouched and keeps the whole feature in new files. It also gives the F3
features a natural home: a group-level row can later carry shared variables
without widening `projects` again.

### DF2-2. URLs

| URL | Meaning |
|---|---|
| `/projects` | Console list of **Projects** with their services grouped under them, plus an "Ungrouped services" section for services with no group. With no groups at all, the page is today's list plus a "Create project" call to action. |
| `/projects/:slug/*` | A **Service**. Unchanged: same path, same page. |
| `/project-groups/:slug/*` | A **Project**: overview, services, settings. |
| `/project-groups` | Redirects to `/projects`. |

- No existing link changes: the web console links to `/projects/:slug/...` in
  hundreds of places and the backend in a few, and every bookmark keeps working.
- **No collision with KV/Blob.** The built-in KV and Blob pages live at
  `/projects/:slug/services/*` (`web/src/pages/ProjectDetail.tsx`, the
  `services/*` route). They sit *under* a service, so they never meet
  `/project-groups/...`. Putting the new entity on `/projects/...` would have
  forced a choice between that path and a Project's own "services" page.
- **The link from a service to its Project is shown by data**, not by URL: the
  sidebar's back link and the breadcrumb look up the service's group
  (`GET /project-groups`, one cached query) and render "← *Project name*" and
  an extra crumb.
- **The sidebar mode is derived from the URL only.** A `/project-groups/:slug`
  path selects the Project sidebar, `/projects/:slug` the Service sidebar, as
  today. No component state remembers a mode, so reload, deep links and the
  browser's back button always agree with what is on screen.

### DF2-3. Permissions

The endpoints reuse the existing project permissions — `ProjectsRead`,
`ProjectsCreate`, `ProjectsWrite`, `ProjectsDelete` — the same choice
`crates/temps-teams` made for its project-access endpoints
(`permission_guard!(auth, ProjectsRead)` / `ProjectsWrite` in
`crates/temps-teams/src/handlers/project_access.rs`). There is **no new
permission and no access grant on a group**.

A group has no access rules of its own. What a caller can see is derived from
the services they can see, through the same `ProjectAccessChecker`
(`crates/temps-core/src/project_access.rs`) that team-gated services already
use. A service is *hidden* from a caller when the checker denies them access to
it. Rules:

1. **Admins** (`Role::Admin`, `Role::PlatformAdmin`) bypass the checker, as in
   `project_access_guard!`.
2. **Deployment tokens get 403** on every `/project-groups` endpoint
   (`deny_deployment_token!`, `crates/temps-auth/src/permission_guard.rs`). A
   deployment token is a machine credential bound to one project; groups are a
   console concern.
3. **Listing and reading** need `ProjectsRead`. A group with at least one member
   in which *every* member is hidden does not appear in the list and returns
   404 on direct reads. A group with no members is visible to anyone holding
   `ProjectsRead`.
4. **Counts and ids only cover visible services.** `service_ids` and
   `service_count` in a response never include hidden services, so names and
   counts of services a user cannot see do not leak through a group.
5. **Create** needs `ProjectsCreate`.
6. **Rename / edit description** needs `ProjectsWrite` and no hidden member. A
   caller who cannot see every member must not change an entity that affects
   services they cannot see: 403.
7. **Delete** needs `ProjectsDelete` and no hidden member: 403 otherwise.
8. **Assign a service** (PUT) needs `ProjectsWrite` and access to *that
   service* (`project_access_guard!` on `project_id`). Without the second check
   a user could pull a service they are gated out of into a group they control.
   **Unassign** needs the same.

The visibility and management decisions are pure functions over
`(groups, members, accessible service ids)` so they are unit-tested without a
database.

### Endpoints and DTOs

OpenAPI tag: `Project Groups`. Timestamps are Unix epoch **milliseconds**, like
the project DTOs (`crates/temps-projects/src/handlers/types.rs` uses
`timestamp_millis()`). Errors are `application/problem+json`
(`temps_core::problemdetails`), the same shape every other handler returns.

| Method and path | Permission | Success | Notes |
|---|---|---|---|
| `GET /project-groups` | `ProjectsRead` | 200 `[ProjectGroupResponse]` | Visible groups only; ordered by name. |
| `POST /project-groups` | `ProjectsCreate` | 201 `ProjectGroupResponse` | Slug generated from the name. |
| `GET /project-groups/{id}` | `ProjectsRead` | 200 `ProjectGroupResponse` | |
| `GET /project-groups/by-slug/{slug}` | `ProjectsRead` | 200 `ProjectGroupResponse` | Mirrors `/projects/by-slug/{slug}` (`crates/temps-projects/src/handlers/handlers.rs`). |
| `PATCH /project-groups/{id}` | `ProjectsWrite` | 200 `ProjectGroupResponse` | Slug is immutable. |
| `DELETE /project-groups/{id}` | `ProjectsDelete` | 204 | Services are kept and become ungrouped. |
| `PUT /project-groups/{id}/projects/{project_id}` | `ProjectsWrite` | 200 `ProjectGroupResponse` | Moves the service if it is in another group. |
| `DELETE /project-groups/{id}/projects/{project_id}` | `ProjectsWrite` | 204 | |

`{id}` and `{project_id}` are the integer ids. `by-slug` is a literal segment,
so it does not collide with `{id}`.

```rust
// Response
ProjectGroupResponse {
    id: i32,
    slug: String,
    name: String,
    description: Option<String>,   // JSON null when unset
    service_ids: Vec<i32>,         // visible members, ascending project id
    service_count: i64,            // == service_ids.len()
    created_at: i64,               // epoch ms
    updated_at: i64,               // epoch ms
}

// POST body
CreateProjectGroupRequest { name: String, description: Option<String> }

// PATCH body: only the fields present are changed
UpdateProjectGroupRequest { name: Option<String>, description: Option<String> }
```

`PUT` and `DELETE` on a membership have no body.

**Create**

```http
POST /api/project-groups
{ "name": "CRM", "description": "Back and front of the internal CRM" }
```

```http
201 Created
{
  "id": 3,
  "slug": "crm",
  "name": "CRM",
  "description": "Back and front of the internal CRM",
  "service_ids": [],
  "service_count": 0,
  "created_at": 1790812800000,
  "updated_at": 1790812800000
}
```

**Assign a service** (`PUT /api/project-groups/3/projects/41`)

```http
200 OK
{
  "id": 3,
  "slug": "crm",
  "name": "CRM",
  "description": "Back and front of the internal CRM",
  "service_ids": [41],
  "service_count": 1,
  "created_at": 1790812800000,
  "updated_at": 1790812860000
}
```

If service 41 was in another group, the same call moves it: it leaves the old
group and joins group 3 in one step. Assigning a service to the group it is
already in is a no-op that returns 200 and writes no audit entry.

**Update.** Only fields present in the body change; an absent field and
`null` both mean "not provided", so clearing the description is done by
sending an empty string.

```http
PATCH /api/project-groups/3
{ "name": "CRM (internal)" }
```

```http
200 OK
{ "id": 3, "slug": "crm", "name": "CRM (internal)", ... }
```

**Error example** (rename of a group with a hidden member; the `detail`
wording is illustrative)

```http
403 Forbidden
Content-Type: application/problem+json

{
  "title": "Project Access Denied",
  "status": 403,
  "detail": "This project includes services you do not have access to"
}
```

| Status | When |
|---|---|
| 400 | Empty or over-long name; invalid body. |
| 401 | No valid credentials. |
| 403 | Missing permission; deployment token; hidden member (rename/delete); no access to the service being assigned or unassigned. |
| 404 | Group not found or fully hidden from the caller; service not found; `DELETE .../projects/{project_id}` when the service is not a member of that group. |
| 409 | The slug generated from the name is already taken. |

### Audit

Each mutation writes an audit entry through the audit service, as the project
handlers do (`AuditOperation` in `crates/temps-projects/src/handlers/audit.rs`).
Failures to write an entry are logged, not returned, matching
`crates/temps-teams/src/handlers/project_access.rs`.

| Operation | Written by | Extra fields |
|---|---|---|
| `PROJECT_GROUP_CREATED` | `POST /project-groups` | `group_id`, `name`, `slug` |
| `PROJECT_GROUP_UPDATED` | `PATCH /project-groups/{id}` | `group_id`, changed fields |
| `PROJECT_GROUP_DELETED` | `DELETE /project-groups/{id}` | `group_id`, `name`, `slug` |
| `PROJECT_GROUP_PROJECT_ASSIGNED` | `PUT .../projects/{project_id}` | `group_id`, `project_id`, `previous_group_id` (null if it had none) |
| `PROJECT_GROUP_PROJECT_REMOVED` | `DELETE .../projects/{project_id}` | `group_id`, `project_id` |

### DF2-5. No plugin events

The group endpoints emit **no plugin events** and do not touch the `project.*`
event contracts: no new event, no new field on an existing one. External
plugins and webhooks see exactly what they saw before. Audit is the only trail.
Nothing in this ADR touches `crates/temps-core/src/jobs.rs` or
`crates/temps-external-plugins`. If a consumer later needs group events, that is
a new, versioned contract and a new ADR.

### DF2-4. The Project badge in the AI workspace

The AI workspace stays a separate entity (ADR-048 rule 6). It gains one
visual cue: where it lists the services linked to an application
(`web/src/components/ai-first/ApplicationProjectsPanel.tsx`,
`RichProjectPicker.tsx`), each service shows a badge with the name of the
Project it belongs to, read from the same cached group list the sidebar uses.
**Web only**: `crates/temps-ai-chat` and the `ai_application_projects` table
(`m20260831_000001_ai_first_applications.rs`) are not modified. A service with
no group shows no badge.

## Non-goals

- **Access grants per group.** Visibility is derived from services (DF2-3);
  there is no "team X may open Project Y".
- **Plugin events** for group changes (DF2-5).
- **Creating a service already inside a Project.** A service is created as
  today and then assigned; there is no `group_id` on the create-project call.
- **A CLI command.** `temps` gets no `project-groups` subcommand in this change.
  (`temps services` already means external services; see ADR-048.)
- **Renaming a group's slug.** The slug is immutable; `PATCH` changes name and
  description only. URLs under `/project-groups/:slug` therefore stay valid.
- **Shared environment variables and PR previews per group.** Both arrive in
  the next phase (F3) and build on `project_group_members` keyed by
  `project_id`: that key is what lets a preview or a variable resolve "which
  group is this service in" with one lookup.

## Consequences

- **Upstream surface is five registration points**, all one-line additions in
  files upstream also edits:
  1. `Cargo.toml` — workspace `members` gets `crates/temps-project-groups`.
  2. `crates/temps-cli/Cargo.toml` — a `temps-project-groups` dependency.
  3. `crates/temps-cli/src/commands/serve/console.rs` — `use` and the plugin
     registration, next to `ProjectsPlugin` ("6. ProjectsPlugin").
  4. `crates/temps-migrations/src/migration/mod.rs` — the `mod` line and the
     entry in the migration list.
  5. `crates/temps-entities/src/lib.rs` — two `pub mod` lines, in a block for
     the fork at the end of the file.

  Everything else is in new files. In the web console, the hot files
  (`Sidebar.tsx`, `Header.tsx`, `App.tsx`, `Projects.tsx`,
  `GeneralSettings.tsx`) receive targeted edits, with logic in new modules.
- **Rollback to an older image is safe.** The migration only creates two tables.
  `run_migrations` (`crates/temps-database/src/connection.rs`, doc comment at
  lines 395-402) states that `Migrator::up` applies only migrations the binary
  defines and that rows in `seaql_migrations` the binary does not know are
  "simply ignored". An older binary therefore starts against a database that
  already holds the extra row and the extra tables, which nothing in the
  existing schema references. The migration's `down` drops both tables if a
  full revert is wanted. No existing table is altered, so `/api/projects` and
  `/api/projects/{id}` return the same bytes before and after.
- **Visibility needs the checker in every read.** Each list and get filters
  through `ProjectAccessChecker`; the pure visibility functions concentrate that
  rule so a new endpoint cannot forget it silently. The
  `every_project_scope_guard_has_access_guard_companion` test
  (`crates/temps-auth/src/permission_guard.rs`) applies to the new handlers too.
- **Generated clients grow.** `web/src/api/client` and the CLI's OpenAPI client
  are regenerated for the new tag; the regeneration is a separate commit so drift
  from the base is not mixed with the feature.
- **F3 builds on this.** Shared variables, per-group PR previews and
  cross-service URLs read `project_group_members` by `project_id`; they need no
  change to `projects`.
