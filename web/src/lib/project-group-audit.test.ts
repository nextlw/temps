// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { describe, expect, it } from 'bun:test'
import { categorize } from '@/components/audit/AuditLogItem'
import { buildOperationOptions } from '@/pages/AuditLogs'
import { i18n } from '@/i18n'
import {
  describeProjectGroupAudit,
  isProjectGroupOperation,
  resolveAuditLabel,
} from './project-group-audit'

describe('project group audit entries', () => {
  it('are their own category, not a service change', () => {
    expect(isProjectGroupOperation('PROJECT_GROUP_CREATED')).toBe(true)
    expect(isProjectGroupOperation('PROJECT_CREATED')).toBe(false)
    expect(categorize('PROJECT_GROUP_PROJECT_ASSIGNED')).toBe('projectGroup')
    expect(categorize('PROJECT_CREATED')).toBe('project')
  })

  it('are filterable, with labels translated when the options are built', () => {
    const seen: string[] = []
    const options = buildOperationOptions((key) => {
      seen.push(key)
      return i18n.t(`audit:${key}`)
    })
    expect(
      options.find(
        (option) => option.value === 'PROJECT_GROUP_PROJECT_ASSIGNED'
      )
    ).toEqual({
      value: 'PROJECT_GROUP_PROJECT_ASSIGNED',
      label: 'Service Added to Project',
      group: 'Projects',
      keywords: 'PROJECT_GROUP_PROJECT_ASSIGNED',
    })
    expect(seen).toContain('ops.projectGroupDeleted')
    expect(seen).toContain('groups.projectGroups')
  })

  it('describe moves, additions and removals by key and values', () => {
    expect(
      describeProjectGroupAudit('PROJECT_GROUP_PROJECT_ASSIGNED', {
        group_id: 3,
        project_id: 41,
        previous_group_id: 2,
      })
    ).toEqual({
      key: 'describe.projectGroupServiceMoved',
      values: { service: '#41', group: '#3', previous: '#2' },
    })
    const added = describeProjectGroupAudit('PROJECT_GROUP_PROJECT_ASSIGNED', {
      group_id: 3,
      project_id: 41,
      previous_group_id: null,
    })
    expect(added?.key).toBe('describe.projectGroupServiceAssigned')
    expect(
      describeProjectGroupAudit('PROJECT_GROUP_CREATED', {
        group_id: 3,
        name: 'CRM',
        slug: 'crm',
      })?.values
    ).toEqual({ group: 'CRM' })
    expect(describeProjectGroupAudit('PROJECT_CREATED', {})).toBeUndefined()
  })

  it('translate with the catalogue', () => {
    const moved = describeProjectGroupAudit('PROJECT_GROUP_PROJECT_ASSIGNED', {
      group_id: 3,
      project_id: 41,
      previous_group_id: 2,
    })!
    expect(i18n.t(`audit:${moved.key}`, moved.values)).toBe(
      'Moved service #41 from project #2 to #3'
    )
    expect(resolveAuditLabel('Plain', () => 'unused')).toBe('Plain')
  })
})
