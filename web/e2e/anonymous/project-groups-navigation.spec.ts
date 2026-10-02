// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { expect, test, type Page } from '@playwright/test'

// Projects (code: project groups, docs/adr/049-project-groups.md) over a
// route-mocked API: the grouped list, a Project's page, a service inside and
// outside a Project, the URLs that existed before Projects, and the browser
// history across the three sidebar levels.

const environment = (projectId: number) => ({
  id: projectId * 10,
  name: 'production',
  slug: 'production',
  project_id: projectId,
  main_url: 'https://example.test',
  subdomain: 'production',
  is_preview: false,
  protected: false,
  sleeping: false,
  current_deployment_id: projectId * 100,
  created_at: 1,
  updated_at: 1,
})

const service = (id: number, name: string, slug: string) => ({
  id,
  name,
  slug,
  preset: 'dockerfile',
  directory: '.',
  main_branch: 'main',
  source_type: 'git',
  project_type: 'application',
  is_public_repo: true,
  git_url: 'https://github.com/example/app',
  repo_owner: 'example',
  repo_name: 'app',
  deployment_config: {},
  created_at: 1,
  updated_at: 1,
})

const backend = service(1, 'CRM backend', 'crm-backend')
const frontend = service(2, 'CRM frontend', 'crm-frontend')
const landing = service(3, 'Landing page', 'landing')
const services = [backend, frontend, landing]

const crm = {
  id: 3,
  slug: 'crm',
  name: 'CRM',
  description: 'Backend and frontend of the internal CRM',
  service_ids: [1, 2],
  service_count: 2,
  created_at: 1,
  updated_at: 1,
}

async function mockApi(page: Page) {
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
    else if (path === '/project-groups') body = [crm]
    else if (path.startsWith('/projects/by-slug/'))
      body = services.find((s) => path === `/projects/by-slug/${s.slug}`)
    else if (/^\/projects\/\d+$/.test(path))
      body = services.find((s) => path === `/projects/${s.id}`)
    else if (path === '/projects')
      body = {
        projects: services,
        total: services.length,
        page: 1,
        per_page: 100,
      }
    else if (/^\/projects\/\d+\/deployments$/.test(path))
      body = { deployments: [], total: 0, page: 1, per_page: 10 }
    else if (/^\/projects\/\d+\/environments$/.test(path))
      body = [environment(Number(path.split('/')[2]))]
    else if (path.endsWith('/last-deployment')) {
      await route.fulfill({ status: 404, json: { detail: 'No deployments' } })
      return
    } else if (path.includes('active-visitors')) body = { count: 0 }
    else if (path.startsWith('/otel/cloud-telemetry')) {
      await route.fulfill({ status: 404, json: { title: 'Not Found' } })
      return
    }
    if (body === undefined) {
      await route.fulfill({ status: 404, json: { title: 'Not Found' } })
      return
    }
    await route.fulfill({ json: body })
  })
}

function watchErrors(page: Page) {
  const errors: string[] = []
  page.on('pageerror', (error) => errors.push(error.message))
  return errors
}

const crumbs = (page: Page) =>
  page.getByRole('navigation', { name: 'breadcrumb', exact: true })

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1000 })
  await mockApi(page)
})

test('the list puts services under their Project and keeps the rest apart', async ({
  page,
}) => {
  const errors = watchErrors(page)
  await page.goto('/projects')
  await expect(
    page.getByRole('heading', { level: 1, name: 'Projects' })
  ).toBeVisible()
  await expect(
    crumbs(page).getByText('Projects', { exact: true })
  ).toBeVisible()

  const card = page.getByRole('article').filter({ hasText: 'CRM' }).first()
  await expect(
    card.getByRole('link', { name: 'CRM', exact: true })
  ).toHaveAttribute('href', '/project-groups/crm')
  const members = card.getByRole('list', { name: 'Services in CRM' })
  await expect(members.getByRole('link')).toHaveText([
    'CRM backend',
    'CRM frontend',
  ])

  const ungrouped = page.getByRole('region', { name: 'Ungrouped services' })
  await expect(ungrouped).toContainText('Landing page')
  await expect(ungrouped).not.toContainText('CRM backend')

  // The search spans Projects and services, by name.
  await page
    .getByRole('textbox', {
      name: 'Filter projects and services by name or slug',
    })
    .fill('landing')
  await expect(page).toHaveURL(/[?&]q=landing/)
  await expect(
    page.getByRole('link', { name: 'CRM', exact: true })
  ).toHaveCount(0)
  await expect(ungrouped).toContainText('Landing page')
  expect(errors).toEqual([])
})

test('a Project lists its services with their state', async ({ page }) => {
  const errors = watchErrors(page)
  await page.goto('/projects')
  await page.getByRole('link', { name: 'CRM', exact: true }).click()
  await expect(page).toHaveURL(/\/project-groups\/crm$/)
  await expect(
    page.getByRole('heading', { level: 1, name: 'CRM' })
  ).toBeVisible()
  const table = page.getByRole('table', { name: 'Services in CRM' })
  await expect(
    table.getByRole('link', { name: 'CRM backend' })
  ).toHaveAttribute('href', '/projects/crm-backend')
  await expect(table).toContainText('Live')
  await expect(table).toContainText('Never deployed')
  await expect(
    page.getByRole('list', { name: 'Project navigation' })
  ).toBeVisible()
  await expect(
    crumbs(page).getByRole('button', { name: 'Switch project' })
  ).toHaveText('CRM')
  expect(errors).toEqual([])
})

test('a service in a Project leads back to it', async ({ page }) => {
  const errors = watchErrors(page)
  await page.goto('/project-groups/crm')
  const table = page.getByRole('table', { name: 'Services in CRM' })
  await table.getByRole('link', { name: 'CRM frontend' }).click()
  await expect(page).toHaveURL(/\/projects\/crm-frontend(\/project)?$/)
  await expect(
    page.getByRole('list', { name: 'Service navigation' })
  ).toBeVisible()
  const back = page.getByRole('link', { name: 'Back to CRM', exact: true })
  await expect(back).toHaveAttribute('href', '/project-groups/crm')
  await expect(
    crumbs(page).getByRole('button', { name: 'Switch project' })
  ).toHaveText('CRM')
  await expect(
    crumbs(page).getByRole('button', { name: 'Switch service' })
  ).toHaveText('CRM frontend')

  await back.click()
  await expect(page).toHaveURL(/\/project-groups\/crm$/)
  await expect(
    page.getByRole('list', { name: 'Project navigation' })
  ).toBeVisible()
  expect(errors).toEqual([])
})

test('a service in no Project leads back to the Projects list', async ({
  page,
}) => {
  const errors = watchErrors(page)
  await page.goto('/projects/landing/environment-variables')
  await expect(
    page.getByRole('list', { name: 'Service navigation' })
  ).toBeVisible()
  await expect(
    page.getByRole('link', { name: 'Back to projects', exact: true })
  ).toHaveAttribute('href', '/projects')
  await expect(
    crumbs(page).getByRole('button', { name: 'Switch project' })
  ).toHaveCount(0)
  await expect(
    crumbs(page).getByRole('link', { name: 'Projects', exact: true })
  ).toHaveAttribute('href', '/projects')
  expect(errors).toEqual([])
})

test('URLs from before Projects still open the same pages', async ({
  page,
}) => {
  const errors = watchErrors(page)
  // A service deep link: same path, now with its Project in the trail.
  await page.goto('/projects/crm-backend/environment-variables')
  await expect(page).toHaveURL(
    /\/projects\/crm-backend\/environment-variables$/
  )
  await expect(
    crumbs(page).getByRole('button', { name: 'Switch service' })
  ).toHaveText('CRM backend')
  await expect(
    crumbs(page).getByRole('button', { name: 'Switch project' })
  ).toHaveText('CRM')

  // `/project-groups` alone is the list, under its old address.
  await page.goto('/project-groups')
  await expect(page).toHaveURL(/\/projects(\?|$)/)
  await expect(
    page.getByRole('heading', { level: 1, name: 'Projects' })
  ).toBeVisible()

  // A Project that is not in the list does not exist.
  await page.goto('/project-groups/unknown')
  await expect(
    page.getByText('Project not found', { exact: true })
  ).toBeVisible()
  await expect(
    page.getByRole('link', { name: 'Back to projects' }).last()
  ).toHaveAttribute('href', '/projects')
  expect(errors).toEqual([])
})

test('back and forward restore page and sidebar together', async ({ page }) => {
  const errors = watchErrors(page)
  const groupNav = page.getByRole('list', { name: 'Project navigation' })
  const serviceNav = page.getByRole('list', { name: 'Service navigation' })

  await page.goto('/projects')
  await page.getByRole('link', { name: 'CRM', exact: true }).click()
  await expect(groupNav).toBeVisible()
  await groupNav.getByRole('link', { name: 'Settings' }).click()
  await expect(page).toHaveURL(/\/project-groups\/crm\/settings$/)
  await expect(
    groupNav.getByRole('link', { name: 'Settings' })
  ).toHaveAttribute('aria-current', 'page')
  await page
    .getByRole('list', { name: 'Services in this project' })
    .getByRole('link', { name: 'CRM backend' })
    .click()
  await expect(serviceNav).toBeVisible()

  await page.goBack()
  await expect(page).toHaveURL(/\/project-groups\/crm\/settings$/)
  await expect(groupNav).toBeVisible()
  await expect(page.getByRole('textbox', { name: 'Name' })).toHaveValue('CRM')
  await page.goBack()
  await expect(page).toHaveURL(/\/project-groups\/crm$/)
  await expect(
    groupNav.getByRole('link', { name: 'Overview' })
  ).toHaveAttribute('aria-current', 'page')
  await page.goBack()
  await expect(page).toHaveURL(/\/projects(\?|$)/)
  await expect(groupNav).toHaveCount(0)
  await expect(serviceNav).toHaveCount(0)

  await page.goForward()
  await page.goForward()
  await page.goForward()
  // The service overview settles on `/projects/:slug/project`.
  await expect(page).toHaveURL(/\/projects\/crm-backend(\/project)?$/)
  await expect(serviceNav).toBeVisible()
  await expect(
    page.getByRole('link', { name: 'Back to CRM', exact: true })
  ).toBeVisible()
  expect(errors).toEqual([])
})

// WCAG 2.4.3: closing a dialog puts focus back on the button that opened it.
test('closing a dialog returns focus to the button that opened it', async ({
  page,
}) => {
  const errors = watchErrors(page)
  await page.goto('/projects')
  const newProject = page.getByRole('button', { name: 'New Project' })
  await newProject.click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(newProject).toBeFocused()

  await page.goto('/project-groups/crm')
  const add = page.getByRole('button', { name: 'Add service' })
  await add.click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(add).toBeFocused()

  const remove = page.getByRole('button', {
    name: 'Remove CRM backend from project',
  })
  await remove.click()
  await expect(page.getByRole('alertdialog')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(remove).toBeFocused()

  await page.goto('/project-groups/crm/settings')
  const deleteProject = page.getByRole('button', { name: 'Delete project' })
  await deleteProject.click()
  await expect(page.getByRole('alertdialog')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(deleteProject).toBeFocused()
  expect(errors).toEqual([])
})

test('with no Project yet, the call to action opens the dialog and gets focus back', async ({
  page,
}) => {
  await page.route('**/api/project-groups', (route) =>
    route.fulfill({ json: [] })
  )
  await page.goto('/projects')
  const create = page.getByRole('button', { name: 'Create project' })
  await create.click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(create).toBeFocused()
})

// Failures are said once, in the console's own words: no app-wide toast
// with the server's English detail (ADR-049 status table).
for (const [status, message] of [
  [409, 'already uses this name'],
  [400, 'Check the name'],
  [403, 'You do not have permission'],
] as const) {
  test(`creating a Project that fails with ${status} explains it in the dialog`, async ({
    page,
  }) => {
    await page.route('**/api/project-groups', (route) =>
      route.request().method() === 'POST'
        ? route.fulfill({
            status,
            json: { title: 'Rejected', detail: 'Server English detail' },
          })
        : route.fallback()
    )
    await page.goto('/projects')
    await page.getByRole('button', { name: 'New Project' }).click()
    const dialog = page.getByRole('dialog')
    const name = dialog.getByRole('textbox', { name: 'Name' })
    await expect(name).toBeFocused()
    await name.fill('CRM')
    await dialog.getByRole('button', { name: 'Create project' }).click()
    await expect(dialog.getByRole('alert')).toContainText(message)
    await expect(page.getByText('Server English detail')).toHaveCount(0)
    await expect(page.locator('[data-sonner-toast]')).toHaveCount(0)
  })
}

test('a 404 on removal reloads the Projects, as its message says', async ({
  page,
}) => {
  await page.route('**/api/project-groups/3/projects/1', (route) =>
    route.fulfill({ status: 404, json: { title: 'Not Found' } })
  )
  let listReads = 0
  page.on('request', (request) => {
    if (
      request.method() === 'GET' &&
      new URL(request.url()).pathname === '/api/project-groups'
    )
      listReads += 1
  })
  await page.goto('/project-groups/crm')
  await page
    .getByRole('button', { name: 'Remove CRM backend from project' })
    .click()
  const before = listReads
  const confirm = page.getByRole('alertdialog')
  await confirm.getByRole('button', { name: 'Remove from project' }).click()
  await expect(confirm.getByRole('alert')).toContainText(
    'The list has been refreshed'
  )
  await expect.poll(() => listReads).toBeGreaterThan(before)
  await expect(page.locator('[data-sonner-toast]')).toHaveCount(0)
})

test('deleting a Project keeps its services and returns to the list', async ({
  page,
}) => {
  let deleted = false
  await page.route('**/api/project-groups**', (route) => {
    const request = route.request()
    const path = new URL(request.url()).pathname
    if (request.method() === 'DELETE' && path === '/api/project-groups/3') {
      deleted = true
      return route.fulfill({ status: 204 })
    }
    if (request.method() === 'GET' && path === '/api/project-groups' && deleted)
      return route.fulfill({ json: [] })
    return route.fallback()
  })
  await page.goto('/project-groups/crm/settings')
  await page.getByRole('button', { name: 'Delete project' }).click()
  const confirm = page.getByRole('alertdialog')
  await expect(confirm).toContainText('Services are kept and become ungrouped')
  await confirm.getByRole('button', { name: 'Delete project' }).click()
  await expect(page).toHaveURL(/\/projects(\?|$)/)
  await expect(page.getByText('CRM deleted')).toBeVisible()
  await expect(page.getByText('CRM backend').first()).toBeVisible()
})

test('"Add service" waits for the services before saying none are left', async ({
  page,
}) => {
  await page.route('**/api/project-groups', (route) =>
    route.fulfill({ json: [{ ...crm, service_ids: [], service_count: 0 }] })
  )
  await page.route('**/api/projects?*', async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 2000))
    await route.fallback()
  })
  await page.goto('/project-groups/crm')
  await page.getByRole('button', { name: 'Add service' }).first().click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('status')).toHaveText('Loading services…')
  await expect(dialog).not.toContainText('already in this project')
  await expect(
    dialog.getByRole('option', { name: /Landing page/ })
  ).toBeVisible()
})

test('on a phone the remove action is on screen without scrolling', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/project-groups/crm')
  const remove = page.getByRole('button', {
    name: 'Remove CRM backend from project',
  })
  await expect(remove).toBeInViewport({ ratio: 1 })
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > window.innerWidth
  )
  expect(overflow).toBe(false)
})

test('creating a Project sends only contract fields and opens it', async ({
  page,
}) => {
  const bodies: unknown[] = []
  await page.route('**/api/project-groups', (route) => {
    if (route.request().method() !== 'POST') return route.fallback()
    bodies.push(route.request().postDataJSON())
    return route.fulfill({
      status: 201,
      json: { ...crm, id: 9, slug: 'data', name: 'Data', service_ids: [] },
    })
  })
  await page.goto('/projects')
  await page.getByRole('button', { name: 'New Project' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('textbox', { name: 'Name' }).fill('  Data  ')
  await dialog.getByRole('button', { name: 'Create project' }).click()
  await expect(page).toHaveURL(/\/project-groups\/data$/)
  expect(bodies).toEqual([{ name: 'Data' }])
})
