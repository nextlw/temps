// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { afterEach, expect, test } from 'bun:test'
import { client } from '@/api/client/client.gen'
import { fetchProjectGroups } from './useProjectGroups'

const originalConfig = client.getConfig()
afterEach(() => client.setConfig(originalConfig))

function respondWith(response: () => Response) {
  const requests: Request[] = []
  client.setConfig({
    baseUrl: 'https://console.example.test/api',
    fetch: Object.assign(
      async (input: RequestInfo | URL) => {
        requests.push(input as Request)
        return response()
      },
      { preconnect: fetch.preconnect }
    ),
  })
  return requests
}

const crm = {
  id: 3,
  slug: 'crm',
  name: 'CRM',
  description: null,
  service_ids: [41],
  service_count: 1,
  created_at: 1,
  updated_at: 1,
}

test('lists the groups from GET /project-groups', async () => {
  const requests = respondWith(() => Response.json([crm]))
  expect(await fetchProjectGroups()).toEqual([crm])
  expect(requests[0]?.method).toBe('GET')
  expect(requests[0]?.url).toBe(
    'https://console.example.test/api/project-groups'
  )
})

test('a server without the endpoint reads as no groups', async () => {
  respondWith(() =>
    Response.json({ title: 'Not Found', status: 404 }, { status: 404 })
  )
  expect(await fetchProjectGroups()).toEqual([])
})

test('an empty or non-list body reads as no groups', async () => {
  respondWith(() => new Response(null, { status: 200 }))
  expect(await fetchProjectGroups()).toEqual([])
  respondWith(() => Response.json({ projects: [] }))
  expect(await fetchProjectGroups()).toEqual([])
})

test('other failures stay errors', async () => {
  respondWith(() =>
    Response.json({ title: 'Forbidden', status: 403 }, { status: 403 })
  )
  await expect(fetchProjectGroups()).rejects.toMatchObject({ status: 403 })
})
