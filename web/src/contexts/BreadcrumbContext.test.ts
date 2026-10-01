// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { describe, expect, it } from 'bun:test'
import {
  type BreadcrumbItem,
  createBreadcrumbOwnership,
} from './BreadcrumbContext-shared'

const layoutTrail = (name: string): BreadcrumbItem[] => [
  { label: 'Services', href: '/projects' },
  { label: name, href: '/projects/web' },
]
const deploymentTrail: BreadcrumbItem[] = [
  { label: 'Services', href: '/projects' },
  { label: 'Web', href: '/projects/web' },
  { label: 'Deployments', href: '/projects/web/deployments' },
  { label: 'Deployment 7' },
]

function setup() {
  let trail: BreadcrumbItem[] = []
  const ownership = createBreadcrumbOwnership((items) => {
    trail = items
  })
  return { ownership, trail: () => trail }
}

describe('breadcrumb ownership', () => {
  it('keeps a nested page trail when its layout loads after it on direct entry', () => {
    const { ownership, trail } = setup()
    const path = '/projects/web/deployments/7'
    // Loading state: only the layout is rendered.
    ownership.setLayout(layoutTrail('Service details'), path)
    expect(trail()).toEqual(layoutTrail('Service details'))
    // Project loaded: the page's effect runs first, then the layout's.
    ownership.setPage(deploymentTrail, path)
    ownership.setLayout(layoutTrail('Web'), path)
    expect(trail()).toEqual(deploymentTrail)
    // Later layout refreshes (refetch, rename) still leave the page's trail.
    ownership.setLayout(layoutTrail('Web renamed'), path)
    expect(trail()).toEqual(deploymentTrail)
  })

  it('lets the layout take the trail back on a URL no page owns', () => {
    const { ownership, trail } = setup()
    ownership.setPage(deploymentTrail, '/projects/web/deployments/7')
    ownership.setLayout(layoutTrail('Web'), '/projects/web/deployments')
    expect(trail()).toEqual(layoutTrail('Web'))
  })

  it('gives the trail to a page reached from a layout-owned URL', () => {
    const { ownership, trail } = setup()
    ownership.setLayout(layoutTrail('Web'), '/projects/web/deployments')
    const path = '/projects/web/deployments/7'
    ownership.setPage(deploymentTrail, path)
    ownership.setLayout(layoutTrail('Web'), path)
    expect(trail()).toEqual(deploymentTrail)
  })

  it('always applies a page trail, even over another page', () => {
    const { ownership, trail } = setup()
    ownership.setPage(deploymentTrail, '/projects/web/deployments/7')
    ownership.setPage([{ label: 'Services' }], '/projects')
    expect(trail()).toEqual([{ label: 'Services' }])
  })
})
