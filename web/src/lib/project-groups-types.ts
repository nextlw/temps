// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Local mirror of the `Project Groups` API DTOs (docs/adr/049-project-groups.md)
// until the generated client in `@/api/client` is regenerated against a server
// that exposes them. Then these types are replaced by the generated ones.
//
// Code names follow ADR-048: `project_group` is the entity the UI calls a
// Project, and its members are the code's `project`s, shown as Services.

/** `ProjectGroupResponse`. Timestamps are Unix epoch milliseconds. */
export interface ProjectGroupResponse {
  id: number
  slug: string
  name: string
  description: string | null
  /**
   * Ids of the member `project`s the caller can see, ascending. A hidden
   * member is never listed, and a listed id may refer to a service that is
   * not on the page of services the console has loaded.
   */
  service_ids: number[]
  /** Equals `service_ids.length`. */
  service_count: number
  created_at: number
  updated_at: number
}

/** `POST /project-groups` body. The slug is derived from the name if absent. */
export interface CreateProjectGroupRequest {
  name: string
  slug?: string
  description?: string | null
}

/**
 * `PATCH /project-groups/{id}` body. Absent fields are left unchanged, and an
 * empty `description` clears it. The slug cannot be changed.
 */
export interface UpdateProjectGroupRequest {
  name?: string
  description?: string
}
