// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { expect, test } from '@playwright/test'

// The sidebar is derived from the URL: leaving a project is a real
// navigation to the list, and browser back/forward restore the matching nav.
test('sidebar follows the URL through back links and browser history', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 })
  const errors: string[] = []
  page.on('pageerror', (error) => errors.push(error.message))
  const environment = {
    id: 1,
    name: 'production',
    slug: 'production',
    project_id: 1,
    domains: [],
    status: 'running',
  }
  const project = {
    id: 1,
    name: 'Example app',
    slug: 'example-app',
    preset: 'docker-compose',
    directory: '.',
    main_branch: 'main',
    source_type: 'git',
    is_public_repo: true,
    git_url: 'https://github.com/example/app',
    repo_owner: 'example',
    repo_name: 'app',
    environments: [environment],
    created_at: 1,
    updated_at: 1,
  }
  await page.route('**/api/**', async (route) => {
    const path = new URL(route.request().url()).pathname.replace(/^\/api/, '')
    let body: unknown = []
    if (path === '/user/me')
      body = {
        id: 1,
        name: 'Owner',
        username: 'owner',
        email: 'owner@example.com',
        role: 'admin',
        mfa_enabled: false,
      }
    // The service is in no Project (project group).
    else if (path === '/project-groups') body = []
    else if (path === '/projects/by-slug/example-app' || path === '/projects/1')
      body = project
    else if (path === '/projects')
      body = { projects: [project], total: 1, page: 1, per_page: 20 }
    else if (path === '/projects/1/environments') body = [environment]
    else if (path.endsWith('/last-deployment')) {
      await route.fulfill({ status: 404, json: { detail: 'No deployments' } })
      return
    } else if (path.includes('active-visitors')) body = { count: 0 }
    await route.fulfill({ json: body })
  })

  await page.goto('/projects/example-app/environment-variables')
  const projectNav = page.getByRole('list', { name: 'Service navigation' })
  await expect(projectNav).toBeVisible()
  // The service is in no Project (`/project-groups` answers `[]`), so its nav
  // leads back to the Projects list.
  const back = page.getByRole('link', { name: 'Back to projects', exact: true })
  await expect(back).toHaveAttribute('href', '/projects')
  const breadcrumbs = page.getByRole('navigation', {
    name: 'breadcrumb',
    exact: true,
  })
  await expect(
    breadcrumbs.getByRole('link', { name: 'Projects', exact: true })
  ).toHaveAttribute('href', '/projects')
  await expect(
    breadcrumbs.getByRole('button', { name: 'Switch service' })
  ).toHaveText('Example app')

  await back.click()
  await expect(page).toHaveURL(/\/projects(\?|$)/)
  await expect(projectNav).toHaveCount(0)
  await expect(page.locator('a[href="/storage"]').first()).toBeVisible()

  await page.goBack()
  await expect(page).toHaveURL(/\/environment-variables$/)
  await expect(projectNav).toBeVisible()
  await page.goForward()
  await expect(page).toHaveURL(/\/projects(\?|$)/)
  await expect(projectNav).toHaveCount(0)

  await page.goto('/settings/users')
  await expect(
    page.getByRole('link', { name: 'Back to main menu', exact: true })
  ).toHaveAttribute('href', '/projects')
  expect(errors).toEqual([])
})
