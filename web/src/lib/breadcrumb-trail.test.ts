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

  it('never takes a link to a Project for the service crumb', () => {
    // A Project named after the service's slug: Projects › web › web.
    expect(
      isServiceCrumb({ label: 'web', href: '/project-groups/web' }, 'web')
    ).toBe(false)
    const trail = withProjectGroupCrumb(
      [
        { label: 'Projects', href: '/projects' },
        { label: 'web', href: '/projects/web' },
      ],
      'web',
      { slug: 'web', name: 'web' }
    )
    expect(trail.map((item) => item.href)).toEqual([
      '/projects',
      '/project-groups/web',
      '/projects/web',
    ])
    const group = { slug: 'web', name: 'web' }
    expect(isProjectGroupCrumb(trail[1], group, false)).toBe(true)
    expect(isServiceCrumb(trail[1], 'web')).toBe(false)
    expect(isServiceCrumb(trail[2], 'web')).toBe(true)
  })

  it('recognises the Project crumb by link on every page', () => {
    const link = { label: 'x', href: '/project-groups/crm' }
    expect(isProjectGroupCrumb(link, crm, false)).toBe(true)
    expect(isProjectGroupCrumb(link, crm, true)).toBe(true)
    expect(
      isProjectGroupCrumb({ label: 'CRM', href: '/projects/crm' }, crm, true)
    ).toBe(false)
  })

  it('matches an unlinked crumb by name only on the group pages', () => {
    // On /project-groups/crm the current page is the unlinked "CRM".
    expect(isProjectGroupCrumb({ label: 'CRM' }, crm, true)).toBe(true)
    expect(isProjectGroupCrumb({ label: 'Settings' }, crm, true)).toBe(false)
    // On a service page, a sub-page named like the Project is not it.
    const settings = { slug: 'settings', name: 'Settings' }
    expect(isProjectGroupCrumb({ label: 'Settings' }, settings, false)).toBe(
      false
    )
    expect(isProjectGroupCrumb({ label: 'CRM' }, crm, false)).toBe(false)
  })

  it('links the encoded slug', () => {
    const cafe = { slug: 'café', name: 'Café' }
    const trail = withProjectGroupCrumb(
      [{ label: 'web', href: '/projects/web' }],
      'web',
      cafe
    )
    expect(trail[0]).toEqual({
      label: 'Café',
      href: '/project-groups/caf%C3%A9',
    })
    expect(isProjectGroupCrumb(trail[0], cafe, false)).toBe(true)
  })
})
