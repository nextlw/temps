// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { describe, expect, it } from 'bun:test'
import type { BreadcrumbItem } from '@/contexts/BreadcrumbContext-shared'
import {
  isProjectGroupCrumb,
  isServiceCrumb,
  withProjectGroupCrumb,
} from './breadcrumb-trail'

const crm = { slug: 'crm', name: 'CRM' }
const deploymentTrail: BreadcrumbItem[] = [
  { label: 'Projects', href: '/projects' },
  { label: 'Web', href: '/projects/web' },
  { label: 'Deployments', href: '/projects/web/deployments' },
  { label: 'Deployment 7' },
]

describe('withProjectGroupCrumb', () => {
  it('puts the Project between the list and the service', () => {
    expect(withProjectGroupCrumb(deploymentTrail, 'web', crm)).toEqual([
      { label: 'Projects', href: '/projects' },
      { label: 'CRM', href: '/project-groups/crm' },
      { label: 'Web', href: '/projects/web' },
      { label: 'Deployments', href: '/projects/web/deployments' },
      { label: 'Deployment 7' },
    ])
  })

  it('finds a service crumb that is the unlinked current page', () => {
    const trail: BreadcrumbItem[] = [
      { label: 'Projects', href: '/projects' },
      { label: 'web' },
    ]
    expect(withProjectGroupCrumb(trail, 'web', crm)).toEqual([
      { label: 'Projects', href: '/projects' },
      { label: 'CRM', href: '/project-groups/crm' },
      { label: 'web' },
    ])
  })

  it('leaves the owner trail alone when there is nothing to add', () => {
    // Ungrouped service, no service in the URL, or the service not in the
    // trail (e.g. a loading layout trail).
    expect(withProjectGroupCrumb(deploymentTrail, 'web', undefined)).toBe(
      deploymentTrail
    )
    expect(withProjectGroupCrumb(deploymentTrail, null, crm)).toBe(
      deploymentTrail
    )
    const loading: BreadcrumbItem[] = [{ label: 'Projects', href: '/projects' }]
    expect(withProjectGroupCrumb(loading, 'web', crm)).toBe(loading)
  })

  it('does not add the Project twice', () => {
    const once = withProjectGroupCrumb(deploymentTrail, 'web', crm)
    expect(withProjectGroupCrumb(once, 'web', crm)).toBe(once)
  })

  it('never mutates the trail the page set', () => {
    const before = structuredClone(deploymentTrail)
    withProjectGroupCrumb(deploymentTrail, 'web', crm)
    expect(deploymentTrail).toEqual(before)
  })
})

describe('crumb matchers', () => {
  it('recognises the service crumb by link or slug label', () => {
    expect(isServiceCrumb({ label: 'Web', href: '/projects/web' }, 'web')).toBe(
      true
    )
    expect(isServiceCrumb({ label: 'web' }, 'web')).toBe(true)
    expect(
      isServiceCrumb({ label: 'Deployments', href: '/projects/web/x' }, 'web')
    ).toBe(false)
  })

  it('recognises the Project crumb by link, or by name when unlinked', () => {
    expect(
      isProjectGroupCrumb({ label: 'x', href: '/project-groups/crm' }, crm)
    ).toBe(true)
    expect(isProjectGroupCrumb({ label: 'CRM' }, crm)).toBe(true)
    expect(
      isProjectGroupCrumb({ label: 'CRM', href: '/projects/crm' }, crm)
    ).toBe(false)
    expect(isProjectGroupCrumb({ label: 'Settings' }, crm)).toBe(false)
  })
})
