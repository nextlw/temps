// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { afterEach, expect, test } from 'bun:test'
import { client } from '@/api/client/client.gen'
import { getProjectsOptions } from '@/api/client/@tanstack/react-query.gen'
import { QueryClient } from '@tanstack/react-query'
import {
  SERVICE_CATALOG_QUERY_KEY,
  fetchServiceCatalog,
} from './useServiceCatalog'

const originalConfig = client.getConfig()
afterEach(() => client.setConfig(originalConfig))

const service = (id: number) => ({ id, name: `svc-${id}`, slug: `svc-${id}` })
const all = Array.from({ length: 250 }, (_, i) => service(i + 1))

function serveCatalog() {
  const pages: string[] = []
  client.setConfig({
    baseUrl: 'https://console.example.test/api',
    fetch: Object.assign(
      async (input: RequestInfo | URL) => {
        const url = new URL((input as Request).url)
        const page = Number(url.searchParams.get('page'))
        const perPage = Number(url.searchParams.get('per_page'))
        pages.push(`${page}/${perPage}`)
        return Response.json({
          projects: all.slice((page - 1) * perPage, page * perPage),
          total: all.length,
          page,
          per_page: perPage,
        })
      },
      { preconnect: fetch.preconnect }
    ),
  })
  return pages
}

test('reads every page, the first one through the shared getProjects entry', async () => {
  const pages = serveCatalog()
  const queryClient = new QueryClient()
  const services = await fetchServiceCatalog(queryClient)
  expect(services.map((s) => s.id)).toEqual(all.map((s) => s.id))
  expect(pages.sort()).toEqual(['1/100', '2/100', '3/100'])
  const shared = queryClient.getQueryData(
    getProjectsOptions({ query: { page: 1, per_page: 100 } }).queryKey
  )
  expect(shared?.projects).toHaveLength(100)
})

test('joins a first-page request already in flight instead of repeating it', async () => {
  const pages = serveCatalog()
  const queryClient = new QueryClient()
  const pending = queryClient.fetchQuery(
    getProjectsOptions({ query: { page: 1, per_page: 100 } })
  )
  await Promise.all([pending, fetchServiceCatalog(queryClient)])
  expect(pages.filter((p) => p === '1/100')).toHaveLength(1)
})

test('lives under the getProjects prefix that service flows invalidate', () => {
  expect(SERVICE_CATALOG_QUERY_KEY[0]).toBe('getProjects')
})
