// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { describe, expect, it } from 'bun:test'
import {
  addableServices,
  deploymentOutcome,
  environmentState,
  servicesOfGroup,
} from './project-group-overview'
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

const catalog = [
  { id: 1, name: 'web' },
  { id: 2, name: 'API' },
  { id: 3, name: 'worker' },
  { id: 4, name: 'billing' },
]

describe('servicesOfGroup', () => {
  it('lists the members found in the catalogue, by name', () => {
    const crm = group(10, 'CRM', [3, 1, 2, 99])
    expect(servicesOfGroup(crm, catalog).map((s) => s.id)).toEqual([2, 1, 3])
  })
  it('is empty for a group without members', () => {
    expect(servicesOfGroup(group(10, 'CRM', []), catalog)).toEqual([])
  })
})

describe('addableServices', () => {
  it('offers every other service, ungrouped first, with the group it leaves', () => {
    const crm = group(10, 'CRM', [1])
    const shop = group(11, 'Shop', [4])
    const result = addableServices([crm, shop], catalog, crm)
    expect(
      result.map((entry) => [entry.service.id, entry.currentGroup?.slug])
    ).toEqual([
      [2, undefined],
      [3, undefined],
      [4, 'shop'],
    ])
  })
  it('never offers a member of the group itself', () => {
    const all = group(10, 'All', [1, 2, 3, 4])
    expect(addableServices([all], catalog, all)).toEqual([])
  })
})

describe('environmentState', () => {
  it('reads sleeping before anything else', () => {
    expect(environmentState({ sleeping: true, current_deployment_id: 7 })).toBe(
      'sleeping'
    )
  })
  it('is live with a current deployment and not deployed without one', () => {
    expect(
      environmentState({ sleeping: false, current_deployment_id: 7 })
    ).toBe('live')
    expect(
      environmentState({ sleeping: false, current_deployment_id: null })
    ).toBe('notDeployed')
    expect(environmentState({ sleeping: false })).toBe('notDeployed')
  })
})

describe('deploymentOutcome', () => {
  it('maps the deployment statuses to five outcomes', () => {
    expect(deploymentOutcome('completed')).toBe('completed')
    expect(deploymentOutcome('FAILED')).toBe('failed')
    expect(deploymentOutcome('cancelled')).toBe('cancelled')
    expect(deploymentOutcome('building')).toBe('inProgress')
    expect(deploymentOutcome('pending')).toBe('inProgress')
    expect(deploymentOutcome('paused')).toBe('other')
  })
})
