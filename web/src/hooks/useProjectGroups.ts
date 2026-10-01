// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Project groups (UI: Projects, docs/adr/049-project-groups.md) through one
// cached list query. The sidebar, the breadcrumb and the switchers all read
// the same entry, and every mutation invalidates it, so a rename or a move
// shows up everywhere without a reload.
//
// The endpoints are called through the generated client's transport (base
// URL, bearer auth, error parsing) with local types until the client is
// regenerated with the `Project Groups` tag.

import { getProjectBySlugOptions } from '@/api/client/@tanstack/react-query.gen'
import { client } from '@/api/client/client.gen'
import { useAuth } from '@/contexts/AuthContext'
import {
  findProjectGroupBySlug,
  groupOfService,
  normalizeProjectGroups,
} from '@/lib/project-groups'
import {
  ProjectGroupRequestError,
  projectGroupErrorReason,
} from '@/lib/project-group-errors'
import { resolveSidebarMode } from '@/lib/sidebar-mode'
import type {
  CreateProjectGroupRequest,
  ProjectGroupResponse,
  UpdateProjectGroupRequest,
} from '@/lib/project-groups-types'
import {
  type QueryClient,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'

const BEARER_SECURITY = [{ scheme: 'bearer', type: 'http' }] as const

/** Prefix of every project-group query; invalidating it refreshes them all. */
export const PROJECT_GROUPS_QUERY_KEY = ['project-groups'] as const

/**
 * `GET /project-groups`. A 404 (a server without the endpoint) and a body that
 * is not a list both read as "no groups"; other failures are errors.
 */
export async function fetchProjectGroups(
  signal?: AbortSignal
): Promise<ProjectGroupResponse[]> {
  const { data, error, response } = await client.get<unknown, unknown, false>({
    security: [...BEARER_SECURITY],
    url: '/project-groups',
    signal,
  })
  if (response?.status === 404) return []
  if (error !== undefined) throw error
  return normalizeProjectGroups(data)
}

/**
 * Every project group the signed-in user can see, ordered as the API returns
 * them (by name). `groups` is `[]` while loading, on error, and when the
 * server has no project groups yet.
 */
export function useProjectGroups() {
  const { user } = useAuth()
  const query = useQuery({
    // Scoped to the account, like ProjectsContext: a different account
    // signing in without a reload never sees the previous one's groups.
    queryKey: [...PROJECT_GROUPS_QUERY_KEY, user?.id] as const,
    queryFn: ({ signal }) => fetchProjectGroups(signal),
    enabled: !!user,
    staleTime: 30_000,
  })
  return { ...query, groups: query.data ?? [] }
}

/**
 * The project group of the page at `pathname`: the group itself on
 * `/project-groups/:slug/*`, the service's group on `/projects/:slug/*`, and
 * nothing elsewhere or for a service in no group. The kind of page comes from
 * the URL, the group from data; the service query is the one the sidebar's
 * service nav already holds.
 */
export function useProjectGroupForPath(pathname: string) {
  const mode = resolveSidebarMode(pathname)
  const serviceSlug = mode.kind === 'project' ? mode.slug : null
  const { groups } = useProjectGroups()
  const { data: service } = useQuery({
    ...getProjectBySlugOptions({ path: { slug: serviceSlug ?? '' } }),
    enabled: serviceSlug !== null,
  })
  if (mode.kind === 'projectGroup') {
    return findProjectGroupBySlug(groups, mode.slug)
  }
  return serviceSlug !== null && service
    ? groupOfService(groups, service.id)
    : undefined
}

export interface ServiceMembership {
  groupId: number
  serviceId: number
}

/**
 * Unwraps a mutation response. A failure throws `ProjectGroupRequestError`
 * with the HTTP status, because the problem body has none, so the caller can
 * say what went wrong in its own (translated) words.
 */
function unwrap<T>(result: {
  data?: T
  error?: unknown
  response?: Response
}): T {
  if (result.error !== undefined || !result.response?.ok) {
    throw new ProjectGroupRequestError(
      result.response?.status ?? 0,
      result.error
    )
  }
  return result.data as T
}

// Every caller shows its own message for a failed mutation (see
// `projectGroupErrorReason`); the app-wide toast would repeat it with the
// server's English detail. A 403 or 404 means the list the screen shows is
// out of date (a group or service gone, access changed), so it is reloaded
// then too, as the 404 message tells the user.
const refreshWhenStale = (invalidate: () => unknown) => (error: Error) => {
  const reason = projectGroupErrorReason(error)
  if (reason === 'notFound' || reason === 'forbidden') void invalidate()
}

/**
 * Options of every project-group mutation for `useMutation` (or a mutation
 * cache): the request, then invalidating the group list so every reader
 * refreshes. A failure throws `ProjectGroupRequestError` and skips the
 * app-wide error toast.
 */
export function projectGroupMutations(queryClient: QueryClient) {
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: PROJECT_GROUPS_QUERY_KEY })
  return {
    create: {
      mutationFn: async (body: CreateProjectGroupRequest) =>
        unwrap(
          await client.post<ProjectGroupResponse, unknown, false>({
            security: [...BEARER_SECURITY],
            url: '/project-groups',
            body,
            headers: { 'Content-Type': 'application/json' },
          })
        ),
      onSuccess: invalidate,
      onError: refreshWhenStale(invalidate),
    },
    update: {
      mutationFn: async ({
        id,
        body,
      }: {
        id: number
        body: UpdateProjectGroupRequest
      }) =>
        unwrap(
          await client.patch<ProjectGroupResponse, unknown, false>({
            security: [...BEARER_SECURITY],
            url: '/project-groups/{id}',
            path: { id },
            body,
            headers: { 'Content-Type': 'application/json' },
          })
        ),
      onSuccess: invalidate,
      onError: refreshWhenStale(invalidate),
    },
    remove: {
      mutationFn: async (id: number) => {
        unwrap(
          await client.delete<unknown, unknown, false>({
            security: [...BEARER_SECURITY],
            url: '/project-groups/{id}',
            path: { id },
          })
        )
      },
      onSuccess: invalidate,
      onError: refreshWhenStale(invalidate),
    },
    assign: {
      mutationFn: async ({ groupId, serviceId }: ServiceMembership) =>
        unwrap(
          await client.put<ProjectGroupResponse, unknown, false>({
            security: [...BEARER_SECURITY],
            url: '/project-groups/{id}/projects/{project_id}',
            path: { id: groupId, project_id: serviceId },
          })
        ),
      onSuccess: invalidate,
      onError: refreshWhenStale(invalidate),
    },
    unassign: {
      mutationFn: async ({ groupId, serviceId }: ServiceMembership) => {
        unwrap(
          await client.delete<unknown, unknown, false>({
            security: [...BEARER_SECURITY],
            url: '/project-groups/{id}/projects/{project_id}',
            path: { id: groupId, project_id: serviceId },
          })
        )
      },
      onSuccess: invalidate,
      onError: refreshWhenStale(invalidate),
    },
  }
}

export function useCreateProjectGroup() {
  return useMutation(projectGroupMutations(useQueryClient()).create)
}

export function useUpdateProjectGroup() {
  return useMutation(projectGroupMutations(useQueryClient()).update)
}

/** Deletes a group. Its services are kept and become ungrouped. */
export function useDeleteProjectGroup() {
  return useMutation(projectGroupMutations(useQueryClient()).remove)
}

/** Puts a service in a group, moving it out of any other group. */
export function useAssignServiceToProjectGroup() {
  return useMutation(projectGroupMutations(useQueryClient()).assign)
}

/** Takes a service out of its group; the service itself is kept. */
export function useRemoveServiceFromProjectGroup() {
  return useMutation(projectGroupMutations(useQueryClient()).unassign)
}
