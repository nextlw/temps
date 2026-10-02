// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { describe, expect, it } from 'bun:test'
import { groupedProjectsView } from './project-groups-list'
import type { ProjectGroupResponse } from '@/api/client'

const group = (
  id: number,
  name: string,
  serviceIds: number[]
): ProjectGroupResponse => ({
  id,
  slug: name.toLowerCase(),
  name,
  description: null,
  service_ids: serviceIds,
  service_count: serviceIds.length,
  created_at: 1,
  updated_at: 1,
})

const svc = (id: number, name: string) => ({ id, name, slug: `svc-${id}` })
const catalog = [
  svc(1, 'web'),
  svc(2, 'api'),
  svc(3, 'landing'),
  svc(4, 'docs'),
  svc(5, 'status page'),
]
const crm = group(10, 'CRM', [2, 1])
const empty = group(11, 'Analytics', [])

const ids = (services: { id: number }[]) => services.map((s) => s.id)

describe('groupedProjectsView', () => {
  it('lists every Project by name, its services by name, and the rest', () => {
    const view = groupedProjectsView([crm, empty], catalog, '', 1, 9)
    expect(view.groups.map((g) => g.group.name)).toEqual(['Analytics', 'CRM'])
    expect(ids(view.groups[1].services)).toEqual([2, 1])
    expect(ids(view.ungroupedPage)).toEqual([3, 4, 5])
    expect(view.ungroupedTotal).toBe(3)
    expect(view.totalPages).toBe(1)
  })

  it('pages only the ungrouped services, in catalogue order', () => {
    const first = groupedProjectsView([crm], catalog, '', 1, 2)
    expect(ids(first.ungroupedPage)).toEqual([3, 4])
    expect(first.totalPages).toBe(2)
    const second = groupedProjectsView([crm], catalog, '', 2, 2)
    expect(ids(second.ungroupedPage)).toEqual([5])
    expect(second.groups).toHaveLength(1)
    expect(groupedProjectsView([crm], catalog, '', 9, 2).ungroupedPage).toEqual(
      []
    )
  })

  it('keeps a Project matching by name whole, else only matching services', () => {
    expect(
      ids(groupedProjectsView([crm], catalog, 'crm', 1, 9).groups[0].services)
    ).toEqual([2, 1])
    const byService = groupedProjectsView([crm, empty], catalog, 'we', 1, 9)
    expect(byService.groups.map((g) => g.group.slug)).toEqual(['crm'])
    expect(ids(byService.groups[0].services)).toEqual([1])
    expect(byService.ungroupedTotal).toBe(0)
  })

  it('does not page while searching', () => {
    const view = groupedProjectsView([crm], catalog, 'a', 2, 1)
    expect(ids(view.ungroupedPage)).toEqual([3, 5])
    expect(view.ungroupedTotal).toBe(2)
  })
})
