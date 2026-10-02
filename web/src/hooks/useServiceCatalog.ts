// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Every service (code: `project`) the signed-in user can see, for the pages
// that place services under their Project (docs/adr/049-project-groups.md).
// A Project's `service_ids` can name any service, so grouping a single page
// of `GET /projects` would drop members that sit on other pages; these pages
// read the whole list instead, 100 at a time (the endpoint's maximum).

import { getProjects, type ProjectResponse } from '@/api/client'
import { getProjectsOptions } from '@/api/client/@tanstack/react-query.gen'
import { useAuth } from '@/contexts/AuthContext'
import {
  useQuery,
  useQueryClient,
  type QueryClient,
} from '@tanstack/react-query'

const PAGE_SIZE = 100
// A ceiling, not an expectation: 50 pages is 5,000 services.
const MAX_PAGES = 50

/**
 * Under the `getProjects` prefix on purpose: the flows that create, import
 * or delete a service invalidate `['getProjects']`, and the catalogue goes
 * stale with them.
 */
export const SERVICE_CATALOG_QUERY_KEY = [
  'getProjects',
  'service-catalog',
] as const

/**
 * Page 1 goes through the generated `getProjects` query (100 per page), the
 * same entry the Project sidebar, the service switcher and the AI workspace
 * read, so a page that shows both asks once. Its `total` gives the number of
 * pages; the others are fetched together.
 */
export async function fetchServiceCatalog(
  queryClient: QueryClient,
  signal?: AbortSignal
): Promise<ProjectResponse[]> {
  const first = await queryClient.fetchQuery({
    ...getProjectsOptions({ query: { page: 1, per_page: PAGE_SIZE } }),
    // Joins a request already in flight; otherwise asks again, since the
    // catalogue only runs when it is stale.
    staleTime: 0,
  })
  const total = first?.total ?? 0
  const pages = Math.min(MAX_PAGES, Math.ceil(total / PAGE_SIZE))
  const rest = await Promise.all(
    Array.from({ length: Math.max(0, pages - 1) }, (_, i) =>
      getProjects({
        query: { page: i + 2, per_page: PAGE_SIZE },
        signal,
        throwOnError: true,
      }).then(({ data }) => data?.projects ?? [])
    )
  )
  return [...(first?.projects ?? []), ...rest.flat()]
}

export function useServiceCatalog({ enabled = true } = {}) {
  const { user } = useAuth()
  const queryClient = useQueryClient()
  const query = useQuery({
    // Scoped to the account, like the groups list.
    queryKey: [...SERVICE_CATALOG_QUERY_KEY, user?.id] as const,
    queryFn: ({ signal }) => fetchServiceCatalog(queryClient, signal),
    enabled: enabled && !!user,
    staleTime: 30_000,
  })
  return { ...query, services: query.data ?? [] }
}
