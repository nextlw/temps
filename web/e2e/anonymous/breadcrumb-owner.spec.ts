// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { expect, test } from '@playwright/test'

// One owner per trail (DESIGN.md): the project layout must not overwrite the
// trail of a page rendered inside it, on direct entry or after its own data
// loads, and must take the trail back when the user leaves that page.
test('deployment detail keeps its own trail on direct entry and hands it back', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 })
  const errors: string[] = []
  page.on('pageerror', (error) => errors.push(error.message))
  const now = Date.now()
  const deployment = {
    id: 3733,
    project_id: 2,
    environment_id: 2,
    environment: { id: 2, name: 'production', slug: 'production', domains: [] },
    status: 'completed',
    is_current: true,
    created_at: now - 3600000,
    started_at: now - 120000,
    finished_at: now - 103000,
    url: 'https://example.test',
    branch: 'main',
    commit_hash: '123456789abcdef',
    commit_message: 'Ship it',
  }
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    let json: unknown = []
    if (path === '/api/user/me')
      json = {
        id: 42,
        name: 'Test Operator',
        username: 'operator',
        email: 'operator@example.com',
        avatar_url: '',
        mfa_enabled: false,
        role: 'admin',
      }
    else if (path === '/api/projects/by-slug/trail-test') {
      // Slow enough that the layout's effect re-runs after the page mounted.
      await new Promise((resolve) => setTimeout(resolve, 300))
      json = {
        id: 2,
        slug: 'trail-test',
        name: 'Trail test',
        source_type: 'git',
        project_type: 'application',
        main_branch: 'main',
        directory: '.',
        created_at: now,
        updated_at: now,
        deployment_config: {},
        preset: 'dockerfile',
      }
    } else if (path.endsWith('/last-deployment')) json = deployment
    else if (path === '/api/projects/2/deployments')
      json = { deployments: [deployment], total: 1 }
    else if (path === '/api/projects/2/deployments/3733') json = deployment
    else if (path.endsWith('/jobs')) json = { jobs: [] }
    else if (path.endsWith('/container-logs')) json = { logs: [] }
    else if (path.endsWith('/environments'))
      json = [
        {
          id: 2,
          name: 'production',
          slug: 'production',
          project_id: 2,
          main_url: 'https://example.test',
          created_at: now,
          updated_at: now,
        },
      ]
    else if (path.endsWith('/active-visitors')) json = { count: 0 }
    await route.fulfill({ json })
  })

  const breadcrumbs = page.getByRole('navigation', {
    name: 'breadcrumb',
    exact: true,
  })
  await page.goto('/projects/trail-test/deployments/3733')
  await expect(
    breadcrumbs.getByRole('link', { name: 'Deployments', exact: true })
  ).toHaveAttribute('href', '/projects/trail-test/deployments')
  await expect(breadcrumbs).toContainText('Deployment 3733')
  await expect(
    breadcrumbs.getByRole('button', { name: 'Switch service' })
  ).toHaveText('Trail test')

  await page.reload()
  await expect(breadcrumbs).toContainText('Deployment 3733')

  await breadcrumbs
    .getByRole('link', { name: 'Deployments', exact: true })
    .click()
  await expect(page).toHaveURL(/\/projects\/trail-test\/deployments$/)
  await expect(breadcrumbs).not.toContainText('Deployment 3733')
  await expect(
    breadcrumbs.getByRole('button', { name: 'Switch service' })
  ).toHaveText('Trail test')
  expect(errors).toEqual([])
})
