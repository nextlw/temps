# ADR-048: UI terminology — Projects contain Services; the code keeps `project`

**Status:** Accepted
**Date:** 2026-09-30
**Scope:** web console (`web/src`) only. No backend, API, CLI, SDK or AI tool
contract changes.

## Context

The console is gaining an entity *above* today's project: a group of
deployable units that ship together (a backend and a frontend, say), with
shared variables and, later, preview environments per pull request. Users
reason about that group as "the project" and about each deployable unit as
"a service" — the vocabulary Railway, Render and Coolify use.

So on screen the new grouping entity must read **Project** and today's
project must read **Service**. The obvious move is to rename in code too. It
is the wrong one for this fork, because "service" is already taken in the
code base and on the wire, in at least these places:

| Existing "service" | Where |
|---|---|
| External services (managed Postgres, Redis, MinIO…) | API `/external-services/*` (`crates/temps-providers`), SDK `createService` / `listServices` (`packages/api`, `sdks/node`), CLI `temps services` (`crates/temps-cli/src/commands/services.rs`); the console labels them "Databases" |
| Built-in KV / Blob per project | `/projects/:slug/services/*` routes (`web/src/pages/ProjectDetail.tsx`), shown as "Platform services" on `/storage` |
| Docker Compose services | `service_name` of a compose project (`crates/temps-presets/src/docker_compose.rs`) and `secret_compose_services` (`crates/temps-deployer/src/compose.rs`) |
| OpenTelemetry | the `service.name` resource attribute (`crates/temps-otel/src/types.rs`), used by traces and logs |
| Service templates | `service_template` / `template_slug` on `projects` (`crates/temps-entities/src/projects.rs`), "Service template" card in project settings |
| AI gateway | "Request/Response Service Tier" (`web/src/pages/AiGateway.tsx`) |

Renaming `project` → `service` in code would overload every one of those,
break every API, CLI and plugin consumer, and — since this is a fork that
merges `gotempsh/temps` regularly — turn each upstream merge into a conflict
across hundreds of files that touch `project`.

## Decision

**Only the interface changes.** The code keeps its names:

| Code (Rust, SQL, API, CLI, plugin SDK, AI tools) | UI (en) |
|---|---|
| `project` | **Service** / Services |
| `project_group` (new entity, defined in [ADR-049](049-project-groups.md)) | **Project** / Projects |
| external service | Database / Databases (`terms.externalService`) |
| KV / Blob under `/projects/:slug/services/*` | Platform resources |
| compose `service_name` | Compose container / Containers (`terms.composeService`) |
| OTel `service.name` | OTel service (`terms.otelService`) |
| `service` field of collected log lines | Container |

Rules that follow:

1. The words come from the `terms` i18n namespace
   (`web/src/i18n/locales/<lng>/terms.json`, read through `useTerms()` in
   `web/src/i18n/terms.ts`). Its keys name the **code entity**:
   `terms.project` is always the code's `project`, whatever it is called on
   screen. Renaming a UI term changes a value, never a key.
2. Other namespaces reference terms by i18next nesting
   (`"$t(terms:project.plural)"`), so one value change propagates to the
   sidebar, breadcrumbs, tooltips and ARIA labels together.
3. URLs keep their paths (`/projects/:slug/...`). Links and bookmarks keep
   working; the words on the page change.
4. The other "service" usages above are always shown with their
   disambiguated label, so "Service" on screen only ever means a deployable
   unit.
5. The `service` field of a collected log line (log explorer, runtime and
   history log viewers) is the Docker label the container was started with
   (`sh.temps.service`, read in
   `crates/temps-log-aggregator/src/services/collector.rs`), not
   the OTel `service.name`. It names the container a line came from, so it
   reads "Container"; "OTel service" is kept for telemetry that really carries
   `service.name` (traces, spans, OpenTelemetry logs and metrics).
6. The AI workspace (`ai_applications`) stays a separate entity. It shows
   which Project a linked service belongs to, but it is neither a Project nor
   a Service.

The swap happens in steps: the i18n infrastructure and the `terms` namespace
land first with today's values (`terms.project` = "Project"); the rename to
"Service" is then a change of values plus the e2e locators that match them.

## Consequences

- API, CLI, SDK, plugins, MCP and AI tools are unaffected; nothing outside
  `web/` needs a migration or a deprecation period.
- Contributors read "Service" on screen and `project` in code. This ADR, and
  the header of `web/src/i18n/terms.ts`, exist so that mismatch is written
  down in the place people look when they hit it.
- Upstream merges stay mechanical in the backend. Conflicts concentrate in
  UI files that upstream also edits (`Sidebar.tsx`, `App.tsx`,
  `CommandPalette.tsx`): when resolving, keep upstream's structure and
  route new user-visible strings through `t()`; never re-introduce a
  hardcoded "Project" for the code's `project`.
- Copy written by upstream after a merge arrives in English and hardcoded;
  it has to be moved into the catalogs (and to `terms` when it names an
  entity) before it is correct for this fork.
- Provider-specific identifiers keep their vendor wording ("Project ID" of
  GCP or Scaleway is not our entity and is not renamed).
