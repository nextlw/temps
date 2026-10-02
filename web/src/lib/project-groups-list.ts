// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// The `/projects` list once Projects exist (docs/adr/049-project-groups.md,
// DF2-2): every Project with its services, then the services in no Project.
// Pure, so search and the paging of "Ungrouped services" are tested without
// rendering.

import { projectPageCount } from './project-list-pagination'
import { groupServices, type ServicesInGroup } from './project-groups'
import type { ProjectGroupResponse } from './project-groups-types'

interface ListedService {
  id: number
  name: string
  slug: string
}

export interface GroupedProjectsView<S> {
  /** Projects to show, by name, each with the services listed under it. */
  groups: ServicesInGroup<S>[]
  /** Ungrouped services matching the search (all of them without one). */
  ungroupedTotal: number
  /** The ungrouped services on the current page (every match when searching). */
  ungroupedPage: S[]
  totalPages: number
}

const matches = (query: string, ...values: string[]) =>
  values.some((value) => value.toLowerCase().includes(query))

const byName = (a: ListedService, b: ListedService) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
  a.id - b.id

/**
 * Splits `catalog` into Projects and ungrouped services, applies the search
 * (`query`, already trimmed and lower-cased) and pages the ungrouped ones.
 *
 * A Project matching by name or slug keeps all its services; otherwise it is
 * shown only with the services that match, and dropped if none do. Without a
 * search every Project is shown, the empty ones too. Ungrouped services keep
 * the catalogue's order, like today's list; while searching they are not
 * paged, again like today's list.
 */
export function groupedProjectsView<S extends ListedService>(
  groups: readonly ProjectGroupResponse[],
  catalog: readonly S[],
  query: string,
  page: number,
  pageSize: number
): GroupedProjectsView<S> {
  const split = groupServices(groups, catalog)
  const shown = split.groups.flatMap(({ group, services }) => {
    const sorted = services.slice().sort(byName)
    if (!query || matches(query, group.name, group.slug)) {
      return [{ group, services: sorted }]
    }
    const hits = sorted.filter((s) => matches(query, s.name, s.slug))
    return hits.length > 0 ? [{ group, services: hits }] : []
  })
  const ungrouped = query
    ? split.ungrouped.filter((s) => matches(query, s.name, s.slug))
    : split.ungrouped
  const totalPages = projectPageCount(ungrouped.length, pageSize)
  const ungroupedPage = query
    ? ungrouped
    : ungrouped.slice((page - 1) * pageSize, page * pageSize)
  return {
    groups: shown,
    ungroupedTotal: ungrouped.length,
    ungroupedPage,
    totalPages,
  }
}
