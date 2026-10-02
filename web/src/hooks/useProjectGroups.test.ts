// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { afterEach, expect, test } from 'bun:test'
import { client } from '@/api/client/client.gen'
import { QueryClient } from '@tanstack/react-query'
import {
  PROJECT_GROUPS_QUERY_KEY,
  fetchProjectGroups,
  projectGroupMutations,
} from './useProjectGroups'

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

// Runs a mutation through a real QueryClient's mutation cache, as useMutation
// does, and reports what was requested and whether the list was invalidated.
async function runMutation<V>(
  pick: (m: ReturnType<typeof projectGroupMutations>) => {
    mutationFn: (v: V) => Promise<unknown>
    onSuccess: () => unknown
  },
  variables: V,
  response: () => Response = () => Response.json(crm)
) {
  const requests = respondWith(response)
  const queryClient = new QueryClient()
  queryClient.setQueryData([...PROJECT_GROUPS_QUERY_KEY, 1], [crm])
  const options = pick(projectGroupMutations(queryClient))
  const result = await queryClient
    .getMutationCache()
    .build(queryClient, options)
    .execute(variables)
  const invalidated =
    queryClient.getQueryState([...PROJECT_GROUPS_QUERY_KEY, 1])
      ?.isInvalidated ?? false
  const request = requests[0]
  const body = request?.body ? await request.json() : undefined
  return { request, body, result, invalidated }
}

const url = (path: string) => `https://console.example.test/api${path}`

test('create POSTs the body and invalidates the list', async () => {
  const run = await runMutation((m) => m.create, {
    name: 'CRM',
    description: 'Back and front',
  })
  expect(run.request?.method).toBe('POST')
  expect(run.request?.url).toBe(url('/project-groups'))
  expect(run.body).toEqual({ name: 'CRM', description: 'Back and front' })
  expect(run.result).toEqual(crm)
  expect(run.invalidated).toBe(true)
})

test('update PATCHes only the given fields of that group', async () => {
  const run = await runMutation((m) => m.update, {
    id: 3,
    body: { description: '' },
  })
  expect(run.request?.method).toBe('PATCH')
  expect(run.request?.url).toBe(url('/project-groups/3'))
  expect(run.body).toEqual({ description: '' })
  expect(run.invalidated).toBe(true)
})

test('delete removes the group', async () => {
  const run = await runMutation(
    (m) => m.remove,
    3,
    () => new Response(null, { status: 204 })
  )
  expect(run.request?.method).toBe('DELETE')
  expect(run.request?.url).toBe(url('/project-groups/3'))
  expect(run.invalidated).toBe(true)
})

test('assign PUTs the membership without a body', async () => {
  const run = await runMutation((m) => m.assign, { groupId: 3, serviceId: 41 })
  expect(run.request?.method).toBe('PUT')
  expect(run.request?.url).toBe(url('/project-groups/3/projects/41'))
  expect(run.body).toBeUndefined()
  expect(run.invalidated).toBe(true)
})

test('unassign DELETEs the membership', async () => {
  const run = await runMutation(
    (m) => m.unassign,
    { groupId: 3, serviceId: 41 },
    () => new Response(null, { status: 204 })
  )
  expect(run.request?.method).toBe('DELETE')
  expect(run.request?.url).toBe(url('/project-groups/3/projects/41'))
  expect(run.invalidated).toBe(true)
})

test('a failed mutation rejects and leaves the list as it was', async () => {
  const queryClient = new QueryClient()
  queryClient.setQueryData([...PROJECT_GROUPS_QUERY_KEY, 1], [crm])
  respondWith(() =>
    Response.json({ title: 'Conflict', status: 409 }, { status: 409 })
  )
  const options = projectGroupMutations(queryClient).create
  await expect(
    queryClient
      .getMutationCache()
      .build(queryClient, options)
      .execute({ name: 'CRM' })
  ).rejects.toMatchObject({ status: 409 })
  expect(
    queryClient.getQueryState([...PROJECT_GROUPS_QUERY_KEY, 1])?.isInvalidated
  ).toBe(false)
})

for (const status of [403, 404]) {
  test(`a ${status} rejects and reloads the list, which is out of date`, async () => {
    const queryClient = new QueryClient()
    queryClient.setQueryData([...PROJECT_GROUPS_QUERY_KEY, 1], [crm])
    respondWith(() => Response.json({ title: 'x' }, { status }))
    const options = projectGroupMutations(queryClient).unassign
    await expect(
      queryClient
        .getMutationCache()
        .build(queryClient, options)
        .execute({ groupId: 3, serviceId: 41 })
    ).rejects.toMatchObject({ status })
    expect(
      queryClient.getQueryState([...PROJECT_GROUPS_QUERY_KEY, 1])?.isInvalidated
    ).toBe(true)
  })
}
