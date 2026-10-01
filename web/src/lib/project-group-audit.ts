// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Audit vocabulary of the Projects (code: project groups,
// docs/adr/049-project-groups.md, "Audit"). The module only holds i18n keys
// and the values to interpolate; the text is translated where it renders, so
// a later language switch is not frozen at import time (F6.0).

import type { ParseKeys } from 'i18next'

export type AuditKey = ParseKeys<'audit'>

/** A label stored as a key, for lists built at module load. */
export interface AuditLabelKey {
  key: AuditKey
}

export type AuditLabel = string | AuditLabelKey

/** Reads a label that may be plain text (legacy entries) or a key. */
export function resolveAuditLabel(
  label: AuditLabel,
  translate: (key: AuditKey) => string
): string {
  return typeof label === 'string' ? label : translate(label.key)
}

export const PROJECT_GROUP_AUDIT_GROUP: AuditLabelKey = {
  key: 'groups.projectGroups',
}

export const PROJECT_GROUP_AUDIT_OPERATIONS = [
  { value: 'PROJECT_GROUP_CREATED', label: { key: 'ops.projectGroupCreated' } },
  { value: 'PROJECT_GROUP_UPDATED', label: { key: 'ops.projectGroupUpdated' } },
  { value: 'PROJECT_GROUP_DELETED', label: { key: 'ops.projectGroupDeleted' } },
  {
    value: 'PROJECT_GROUP_PROJECT_ASSIGNED',
    label: { key: 'ops.projectGroupServiceAssigned' },
  },
  {
    value: 'PROJECT_GROUP_PROJECT_REMOVED',
    label: { key: 'ops.projectGroupServiceRemoved' },
  },
] as const satisfies readonly { value: string; label: AuditLabelKey }[]

export function isProjectGroupOperation(op: string): boolean {
  return op.startsWith('PROJECT_GROUP_')
}

export interface AuditDescription {
  key: AuditKey
  values: Record<string, string | number>
}

const text = (data: Record<string, unknown> | undefined, key: string) => {
  const value = data?.[key]
  return typeof value === 'string' && value.trim() ? value : undefined
}

const id = (data: Record<string, unknown> | undefined, key: string) => {
  const value = data?.[key]
  return typeof value === 'number' || typeof value === 'string'
    ? String(value)
    : undefined
}

/**
 * The sentence for a `PROJECT_GROUP_*` entry, from the fields the ADR lists
 * (`group_id`, `name`, `slug`, `project_id`, `previous_group_id`). Names are
 * used when the entry has them, ids otherwise. `undefined` for any other
 * operation.
 */
export function describeProjectGroupAudit(
  op: string,
  data?: Record<string, unknown>
): AuditDescription | undefined {
  const name = text(data, 'name') ?? text(data, 'group_name')
  const group = name ?? `#${id(data, 'group_id') ?? '?'}`
  const service =
    text(data, 'project_name') ??
    text(data, 'project_slug') ??
    `#${id(data, 'project_id') ?? '?'}`
  switch (op) {
    case 'PROJECT_GROUP_CREATED':
      return { key: 'describe.projectGroupCreated', values: { group } }
    case 'PROJECT_GROUP_UPDATED':
      return { key: 'describe.projectGroupUpdated', values: { group } }
    case 'PROJECT_GROUP_DELETED':
      return { key: 'describe.projectGroupDeleted', values: { group } }
    case 'PROJECT_GROUP_PROJECT_ASSIGNED': {
      const previous = id(data, 'previous_group_id')
      return previous
        ? {
            key: 'describe.projectGroupServiceMoved',
            values: { service, group, previous: `#${previous}` },
          }
        : {
            key: 'describe.projectGroupServiceAssigned',
            values: { service, group },
          }
    }
    case 'PROJECT_GROUP_PROJECT_REMOVED':
      return {
        key: 'describe.projectGroupServiceRemoved',
        values: { service, group },
      }
    default:
      return undefined
  }
}
