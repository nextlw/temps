// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Pure helpers over project groups (docs/adr/049-project-groups.md): the UI's
// Projects, each grouping some of the code's `project`s (the UI's Services).
//
// The API only lists what the caller can see: a group whose members are all
// hidden is absent, and `service_ids` never names a hidden service. These
// helpers therefore treat "not in the list" as "does not exist", and only
// place a service under a group when that service is in the given list too.

import type { ProjectGroupResponse } from '@/api/client'

/** Base path of the Project pages (`/project-groups/:slug/*`). */
export const PROJECT_GROUPS_PATH = '/project-groups'

export function projectGroupHref(slug: string): string {
  return `${PROJECT_GROUPS_PATH}/${encodeURIComponent(slug)}`
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null

const isProjectGroup = (value: unknown): value is ProjectGroupResponse =>
  isRecord(value) &&
  typeof value.id === 'number' &&
  typeof value.slug === 'string' &&
  typeof value.name === 'string' &&
  Array.isArray(value.service_ids) &&
  value.service_ids.every((id) => typeof id === 'number')

/**
 * Reads a `GET /project-groups` body. Anything but a list of groups (an empty
 * body, an HTML fallback page, a server without the endpoint) reads as "no
 * groups", so the console keeps working against a backend without them.
 */
export function normalizeProjectGroups(data: unknown): ProjectGroupResponse[] {
  if (!Array.isArray(data)) return []
  return data.filter(isProjectGroup)
}

const byName = (a: ProjectGroupResponse, b: ProjectGroupResponse) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
  a.slug.localeCompare(b.slug)

/** Groups ordered by name, then slug; the input is left untouched. */
export function sortProjectGroups(
  groups: readonly ProjectGroupResponse[]
): ProjectGroupResponse[] {
  return groups.slice().sort(byName)
}

export function findProjectGroupBySlug(
  groups: readonly ProjectGroupResponse[],
  slug: string
): ProjectGroupResponse | undefined {
  return groups.find((group) => group.slug === slug)
}

/**
 * The group a service belongs to, if the caller can see one. A service is in
 * at most one group; should two list it anyway, the first by name wins, the
 * same choice `groupServices` makes.
 */
export function groupOfService(
  groups: readonly ProjectGroupResponse[],
  serviceId: number
): ProjectGroupResponse | undefined {
  return sortProjectGroups(groups).find((group) =>
    group.service_ids.includes(serviceId)
  )
}

export interface ServicesInGroup<S> {
  group: ProjectGroupResponse
  services: S[]
}

export interface GroupedServices<S> {
  /** Every group, by name, with the given services that belong to it. */
  groups: ServicesInGroup<S>[]
  /** Given services that belong to no visible group, in input order. */
  ungrouped: S[]
}

/**
 * Splits `services` by group. Services keep their input order inside each
 * bucket, so the caller decides how they sort. A group's ids that are not in
 * `services` (a service on another page of the list) are skipped, which can
 * leave a group with no services here: callers that only list services drop
 * such groups, a list of Projects keeps them.
 */
export function groupServices<S extends { id: number }>(
  groups: readonly ProjectGroupResponse[],
  services: readonly S[]
): GroupedServices<S> {
  const claimed = new Set<number>()
  const buckets = sortProjectGroups(groups).map((group) => {
    const members = new Set(group.service_ids)
    const inGroup = services.filter(
      (service) => members.has(service.id) && !claimed.has(service.id)
    )
    for (const service of inGroup) claimed.add(service.id)
    return { group, services: inGroup }
  })
  return {
    groups: buckets,
    ungrouped: services.filter((service) => !claimed.has(service.id)),
  }
}
