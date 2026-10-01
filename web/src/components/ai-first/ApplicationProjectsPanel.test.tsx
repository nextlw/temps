// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from '@/i18n/testing'
import { MemoryRouter } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  listProjectServicesOptions,
  listServicesOptions,
} from '@/api/client/@tanstack/react-query.gen'
import type { ApplicationResponse, ExternalServiceInfo } from '@/api/client'
import { AuthContext } from '@/contexts/AuthContext-shared'
import { PROJECT_GROUPS_QUERY_KEY } from '@/hooks/useProjectGroups'
import type { ReactNode } from 'react'
import { ApplicationProjectsPanel } from './ApplicationProjectsPanel'

const owner = {
  user: { id: 1 } as never,
  isLoading: false,
  error: null,
  logout: async () => {},
  refetch: () => {},
}

// The panel reads the signed-in user's Projects (project groups) to badge
// the linked services.
function Providers({
  client,
  children,
}: {
  client: QueryClient
  children: ReactNode
}) {
  return (
    <QueryClientProvider client={client}>
      <AuthContext.Provider value={owner}>
        <MemoryRouter>{children}</MemoryRouter>
      </AuthContext.Provider>
    </QueryClientProvider>
  )
}

describe('ApplicationProjectsPanel', () => {
  test('renders primary, deployment, and automatic-deploy state', () => {
    const queryClient = new QueryClient()
    const html = renderToStaticMarkup(
      <Providers client={queryClient}>
        <ApplicationProjectsPanel
          application={application}
          onApplicationChange={() => {}}
        />
      </Providers>
    )

    expect(html).toContain('Primary')
    expect(html).toContain('Not deployed yet')
    expect(html).toContain('Automatic deploy')
    expect(html).toContain('Disabled')
    expect(html).toContain('Production')
    expect(html).toContain('ready')
    expect(html).toContain('/projects/web')
    expect(html).toContain('An application must keep at least one service')
    expect(html).toContain('Service topology is injected fresh')
  })

  test('shows linked databases and creates new ones atomically for the primary project', () => {
    const queryClient = new QueryClient()
    queryClient.setQueryData(listServicesOptions().queryKey, [postgres])
    queryClient.setQueryData(
      listProjectServicesOptions({ path: { project_id: 7 } }).queryKey,
      [
        {
          id: 12,
          database_provisioning_mode: 'project_environment',
          project: {
            id: 7,
            slug: 'web',
            created_at: '2026-09-03T00:00:00Z',
          },
          service: postgres,
        },
      ]
    )

    const html = renderToStaticMarkup(
      <Providers client={queryClient}>
        <ApplicationProjectsPanel
          application={application}
          onApplicationChange={() => {}}
        />
      </Providers>
    )

    expect(html).toContain('Databases')
    expect(html).toContain('main-postgres')
    expect(html).toContain('PostgreSQL 18 · private network')
    expect(html).toContain('/storage/create?project_id=7')
    expect(html).toContain('sandbox receives no reusable platform token')
  })

  test('badges each linked service with its Project, and only then', () => {
    const queryClient = new QueryClient()
    queryClient.setQueryData(
      [...PROJECT_GROUPS_QUERY_KEY, 1],
      [
        {
          id: 3,
          slug: 'crm',
          name: 'CRM',
          description: null,
          service_ids: [7],
          service_count: 1,
          created_at: 1,
          updated_at: 1,
        },
      ]
    )
    const html = renderToStaticMarkup(
      <Providers client={queryClient}>
        <ApplicationProjectsPanel
          application={application}
          onApplicationChange={() => {}}
        />
      </Providers>
    )
    expect(html).toContain('Project: CRM')

    const ungrouped = renderToStaticMarkup(
      <Providers client={new QueryClient()}>
        <ApplicationProjectsPanel
          application={application}
          onApplicationChange={() => {}}
        />
      </Providers>
    )
    expect(ungrouped).not.toContain('Project:')
  })

  test('requires a project before offering database links', () => {
    const queryClient = new QueryClient()
    const html = renderToStaticMarkup(
      <Providers client={queryClient}>
        <ApplicationProjectsPanel
          application={{ ...application, projects: [] }}
          onApplicationChange={() => {}}
        />
      </Providers>
    )

    expect(html).toContain('Add a service first')
    expect(html).toContain(
      'Databases are linked through an application service'
    )
    expect(html).not.toContain('Choose a database')
  })
})

const application: ApplicationResponse = {
  public_id: 'app_test',
  name: 'Test application',
  description: null,
  status: 'active',
  created_at: '2026-09-03T00:00:00Z',
  updated_at: '2026-09-03T00:00:00Z',
  projects: [
    {
      id: 7,
      name: 'Web',
      slug: 'web',
      repository: '/',
      main_branch: 'main',
      is_private: true,
      is_primary: true,
      automatic_deploy: false,
      last_deployment_at: null,
      environments: [
        {
          name: 'Production',
          slug: 'production',
          sleeping: false,
          deployment_state: 'ready',
        },
      ],
    },
  ],
}

const postgres: ExternalServiceInfo = {
  id: 4,
  name: 'main-postgres',
  service_type: 'postgres',
  status: 'running',
  topology: 'standalone',
  version: '18',
  created_at: '2026-09-03T00:00:00Z',
  updated_at: '2026-09-03T00:00:00Z',
}
