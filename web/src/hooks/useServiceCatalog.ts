// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Every service (code: `project`) the signed-in user can see, for the pages
// that place services under their Project (docs/adr/049-project-groups.md).
// A Project's `service_ids` can name any service, so grouping a single page
// of `GET /projects` would drop members that sit on other pages; these pages
// read the whole list instead, 100 at a time (the endpoint's maximum).

import { getProjects, type ProjectResponse } from '@/api/client'
import { useAuth } from '@/contexts/AuthContext'
import { useQuery } from '@tanstack/react-query'

const PAGE_SIZE = 100
// A ceiling, not an expectation: 50 pages is 5,000 services.
const MAX_PAGES = 50

export const SERVICE_CATALOG_QUERY_KEY = ['service-catalog'] as const

export async function fetchServiceCatalog(
  signal?: AbortSignal
): Promise<ProjectResponse[]> {
  const services: ProjectResponse[] = []
  for (let page = 1; page <= MAX_PAGES; page += 1) {
    const { data } = await getProjects({
      query: { page, per_page: PAGE_SIZE },
      signal,
      throwOnError: true,
    })
    const batch = data?.projects ?? []
    services.push(...batch)
    if (batch.length < PAGE_SIZE || services.length >= (data?.total ?? 0)) {
      break
    }
  }
  return services
}

export function useServiceCatalog({ enabled = true } = {}) {
  const { user } = useAuth()
  const query = useQuery({
    // Scoped to the account, like the groups list.
    queryKey: [...SERVICE_CATALOG_QUERY_KEY, user?.id] as const,
    queryFn: ({ signal }) => fetchServiceCatalog(signal),
    enabled: enabled && !!user,
    staleTime: 30_000,
  })
  return { ...query, services: query.data ?? [] }
}
