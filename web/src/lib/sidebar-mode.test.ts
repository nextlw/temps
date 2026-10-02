// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { describe, expect, it } from 'bun:test'
import {
  SIDEBAR_BACK_TARGET,
  projectNavBackTarget,
  resolveProjectGroupSection,
  resolveSidebarMode,
  type SidebarMode,
} from './sidebar-mode'
import { WORKER_NODES_URL } from './worker-nodes'

const project = (slug: string): SidebarMode => ({ kind: 'project', slug })
const projectGroup = (slug: string): SidebarMode => ({
  kind: 'projectGroup',
  slug,
})
const DEFAULT: SidebarMode = { kind: 'default' }

describe('resolveSidebarMode', () => {
  it('enters the project nav on any page inside a project', () => {
    expect(resolveSidebarMode('/projects/web-app')).toEqual(project('web-app'))
    expect(resolveSidebarMode('/projects/web-app/deployments')).toEqual(
      project('web-app')
    )
    expect(resolveSidebarMode('/projects/web-app/deployments/42')).toEqual(
      project('web-app')
    )
  })

  it('keeps the default nav on the project list and creation routes', () => {
    expect(resolveSidebarMode('/projects')).toEqual(DEFAULT)
    expect(resolveSidebarMode('/projects/new')).toEqual(DEFAULT)
    expect(resolveSidebarMode('/projects/import')).toEqual(DEFAULT)
    expect(resolveSidebarMode('/projects/import-wizard')).toEqual(DEFAULT)
  })

  it('going back from a project lands on the list, which shows the default nav', () => {
    const inside = resolveSidebarMode('/projects/web-app/deployments')
    expect(inside.kind).toBe('project')
    expect(SIDEBAR_BACK_TARGET.project).toBe('/projects')
    expect(resolveSidebarMode(SIDEBAR_BACK_TARGET.project)).toEqual(DEFAULT)
  })

  it('shows the default nav on global items, including their project-like names', () => {
    expect(resolveSidebarMode('/storage')).toEqual(DEFAULT)
    expect(resolveSidebarMode('/domains')).toEqual(DEFAULT)
    expect(resolveSidebarMode('/logs')).toEqual(DEFAULT)
    // A project's own Databases page is still the project nav.
    expect(resolveSidebarMode('/projects/web-app/storage')).toEqual(
      project('web-app')
    )
  })

  it('follows browser back/forward: the same URL always gives the same nav', () => {
    // Visit a project, go to the list, a global page, then walk history back.
    const history = [
      '/projects/web-app/deployments',
      '/projects',
      '/storage',
      '/projects',
      '/projects/web-app/deployments',
    ]
    const modes = history.map(resolveSidebarMode)
    expect(modes).toEqual([
      project('web-app'),
      DEFAULT,
      DEFAULT,
      DEFAULT,
      project('web-app'),
    ])
    // Order-independent: resolving in reverse yields the reverse.
    expect([...history].reverse().map(resolveSidebarMode)).toEqual(
      [...modes].reverse()
    )
  })

  it('treats only the reserved segments as non-project routes', () => {
    expect(resolveSidebarMode('/projects/')).toEqual(DEFAULT)
    expect(resolveSidebarMode('/projects/importer')).toEqual(
      project('importer')
    )
    expect(resolveSidebarMode('/projects/newsletter/deployments')).toEqual(
      project('newsletter')
    )
  })

  it('decodes the slug, keeping a malformed escape as-is', () => {
    expect(resolveSidebarMode('/projects/caf%C3%A9/deployments')).toEqual(
      project('café')
    )
    expect(resolveSidebarMode('/projects/100%/deployments')).toEqual(
      project('100%')
    )
  })

  it('matches area prefixes by whole path segment', () => {
    expect(resolveSidebarMode('/settingsfoo')).toEqual(DEFAULT)
    expect(resolveSidebarMode('/chatbots')).toEqual(DEFAULT)
    expect(resolveSidebarMode('/skillset')).toEqual(DEFAULT)
    expect(resolveSidebarMode(`${WORKER_NODES_URL}/abc`)).toEqual(DEFAULT)
    expect(resolveSidebarMode('/settings/nodesx')).toEqual({
      kind: 'settings',
    })
  })

  it('swaps to settings and AI navs, with real back targets', () => {
    expect(resolveSidebarMode('/settings')).toEqual({ kind: 'settings' })
    expect(resolveSidebarMode('/settings/users')).toEqual({ kind: 'settings' })
    expect(resolveSidebarMode(WORKER_NODES_URL)).toEqual(DEFAULT)
    expect(resolveSidebarMode('/ai-gateway/usage')).toEqual({ kind: 'ai' })
    expect(resolveSidebarMode('/chat')).toEqual({ kind: 'ai' })
    expect(SIDEBAR_BACK_TARGET.settings).toBe('/projects')
    expect(resolveSidebarMode(SIDEBAR_BACK_TARGET.settings)).toEqual(DEFAULT)
    expect(resolveSidebarMode(SIDEBAR_BACK_TARGET.ai)).toEqual(DEFAULT)
  })
})

describe('project groups in the sidebar', () => {
  it('enters the Project nav on any page inside a project group', () => {
    expect(resolveSidebarMode('/project-groups/crm')).toEqual(
      projectGroup('crm')
    )
    expect(resolveSidebarMode('/project-groups/crm/settings')).toEqual(
      projectGroup('crm')
    )
    expect(resolveSidebarMode('/project-groups/caf%C3%A9')).toEqual(
      projectGroup('café')
    )
  })

  it('keeps the default nav on /project-groups itself and look-alikes', () => {
    expect(resolveSidebarMode('/project-groups')).toEqual(DEFAULT)
    expect(resolveSidebarMode('/project-groups/')).toEqual(DEFAULT)
    expect(resolveSidebarMode('/project-groupsx/crm')).toEqual(DEFAULT)
  })

  it('leaves a project group for the Projects list, which shows the default nav', () => {
    expect(SIDEBAR_BACK_TARGET.projectGroup).toBe('/projects')
    expect(resolveSidebarMode(SIDEBAR_BACK_TARGET.projectGroup)).toEqual(
      DEFAULT
    )
  })

  it('keeps the service nav for a service, whatever group it is in', () => {
    // The group is data: the URL alone still selects the service nav.
    expect(resolveSidebarMode('/projects/web-app/deployments')).toEqual(
      project('web-app')
    )
  })

  it('backs out of a service to its group, or to the list without one', () => {
    expect(projectNavBackTarget({ slug: 'crm' })).toBe('/project-groups/crm')
    expect(projectNavBackTarget(undefined)).toBe(SIDEBAR_BACK_TARGET.project)
    expect(resolveSidebarMode(projectNavBackTarget({ slug: 'crm' }))).toEqual(
      projectGroup('crm')
    )
  })

  it('walks service → group → list → back again from the URL alone', () => {
    const history = [
      '/projects/web-app/deployments',
      '/project-groups/crm',
      '/projects',
      '/project-groups/crm',
      '/projects/web-app/deployments',
    ]
    expect(history.map(resolveSidebarMode)).toEqual([
      project('web-app'),
      projectGroup('crm'),
      DEFAULT,
      projectGroup('crm'),
      project('web-app'),
    ])
  })

  it('maps project group paths to their nav entry', () => {
    expect(resolveProjectGroupSection('/project-groups/crm')).toBe('overview')
    expect(resolveProjectGroupSection('/project-groups/crm/')).toBe('overview')
    expect(resolveProjectGroupSection('/project-groups/crm/settings')).toBe(
      'settings'
    )
    expect(
      resolveProjectGroupSection('/project-groups/crm/settings/danger')
    ).toBe('settings')
    expect(resolveProjectGroupSection('/project-groups/crm/settingsx')).toBe(
      null
    )
    expect(resolveProjectGroupSection('/project-groups/crm/other')).toBe(null)
    expect(resolveProjectGroupSection('/projects/crm')).toBe(null)
  })
})
