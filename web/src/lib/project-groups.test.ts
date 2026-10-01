// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { describe, expect, it } from 'bun:test'
import {
  findProjectGroupBySlug,
  groupOfService,
  groupServices,
  normalizeProjectGroups,
  projectGroupHref,
  sortProjectGroups,
} from './project-groups'
import type { ProjectGroupResponse } from './project-groups-types'

const group = (
  id: number,
  name: string,
  serviceIds: number[],
  slug = name.toLowerCase()
): ProjectGroupResponse => ({
  id,
  slug,
  name,
  description: null,
  service_ids: serviceIds,
  service_count: serviceIds.length,
  created_at: 1,
  updated_at: 1,
})

const service = (id: number) => ({ id, name: `svc-${id}` })

describe('projectGroupHref', () => {
  it('points at the Project pages, not the service ones', () => {
    expect(projectGroupHref('crm')).toBe('/project-groups/crm')
  })
})

describe('normalizeProjectGroups', () => {
  it('reads anything but a list as no groups', () => {
    expect(normalizeProjectGroups(undefined)).toEqual([])
    expect(normalizeProjectGroups(null)).toEqual([])
    expect(normalizeProjectGroups('')).toEqual([])
    expect(normalizeProjectGroups('<!doctype html>')).toEqual([])
    expect(normalizeProjectGroups({ detail: 'Not Found' })).toEqual([])
  })

  it('keeps well-formed groups and drops malformed entries', () => {
    const crm = group(1, 'CRM', [2, 3])
    expect(
      normalizeProjectGroups([
        crm,
        null,
        { id: 2, slug: 'x' },
        { ...group(3, 'Bad', []), service_ids: ['1'] },
      ])
    ).toEqual([crm])
  })
})

describe('sortProjectGroups / findProjectGroupBySlug', () => {
  it('orders by name ignoring case, then slug, without mutating', () => {
    const groups = [
      group(1, 'web', []),
      group(2, 'Api', []),
      group(3, 'api', [], 'api-2'),
    ]
    expect(sortProjectGroups(groups).map((g) => g.id)).toEqual([2, 3, 1])
    expect(groups.map((g) => g.id)).toEqual([1, 2, 3])
  })

  it('finds a group by its slug only', () => {
    const groups = [group(1, 'CRM', [], 'crm')]
    expect(findProjectGroupBySlug(groups, 'crm')?.id).toBe(1)
    expect(findProjectGroupBySlug(groups, 'CRM')).toBeUndefined()
    expect(findProjectGroupBySlug([], 'crm')).toBeUndefined()
  })
})

describe('groupOfService', () => {
  const groups = [group(1, 'Web', [5]), group(2, 'CRM', [2, 3])]

  it('returns the group listing the service', () => {
    expect(groupOfService(groups, 3)?.slug).toBe('crm')
    expect(groupOfService(groups, 5)?.slug).toBe('web')
  })

  it('returns nothing for an ungrouped service or no groups', () => {
    expect(groupOfService(groups, 9)).toBeUndefined()
    expect(groupOfService([], 3)).toBeUndefined()
  })

  it('picks the first group by name if two list the same service', () => {
    const overlapping = [group(1, 'Zeta', [7]), group(2, 'Alpha', [7])]
    expect(groupOfService(overlapping, 7)?.name).toBe('Alpha')
  })
})

describe('groupServices', () => {
  it('splits services into groups by name and keeps the rest ungrouped', () => {
    const services = [service(1), service(2), service(3), service(4)]
    const result = groupServices(
      [group(1, 'Web', [4]), group(2, 'CRM', [3, 1])],
      services
    )
    expect(
      result.groups.map((g) => [g.group.name, g.services.map((s) => s.id)])
    ).toEqual([
      ['CRM', [1, 3]],
      ['Web', [4]],
    ])
    expect(result.ungrouped.map((s) => s.id)).toEqual([2])
  })

  it('keeps the input order of services inside each bucket', () => {
    const services = [service(3), service(1), service(2)]
    const result = groupServices([group(1, 'CRM', [1, 2, 3])], services)
    expect(result.groups[0].services.map((s) => s.id)).toEqual([3, 1, 2])
  })

  it('skips ids of services that are not in the given list', () => {
    // A member on another page of services, or hidden from the caller, is
    // never invented here; the group stays, without it.
    const result = groupServices(
      [group(1, 'CRM', [1, 99]), group(2, 'Empty', [])],
      [service(1)]
    )
    expect(
      result.groups.map((g) => [g.group.name, g.services.map((s) => s.id)])
    ).toEqual([
      ['CRM', [1]],
      ['Empty', []],
    ])
    expect(result.ungrouped).toEqual([])
  })

  it('puts a service listed twice in one group only', () => {
    const result = groupServices(
      [group(1, 'Zeta', [7]), group(2, 'Alpha', [7])],
      [service(7)]
    )
    expect(result.groups.map((g) => g.services.length)).toEqual([1, 0])
    expect(result.groups[0].group.name).toBe('Alpha')
  })

  it('leaves every service ungrouped without groups', () => {
    const services = [service(1), service(2)]
    expect(groupServices([], services)).toEqual({
      groups: [],
      ungrouped: services,
    })
  })
})
