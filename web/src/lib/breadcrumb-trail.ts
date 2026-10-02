// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Render-time additions to the shared breadcrumb trail. Pages own their trail
// (one owner per trail, DESIGN.md "Shared breadcrumbs") and keep building it
// as `Projects › <service> › …`. Which Project a service is in is data the
// pages do not load, so the header adds that crumb when it renders, without
// touching what the owner set.

import type { BreadcrumbItem } from '@/contexts/BreadcrumbContext-shared'
import { PROJECT_GROUPS_PATH, projectGroupHref } from '@/lib/project-groups'

export interface CrumbGroup {
  slug: string
  name: string
}

/**
 * The crumb naming the service `serviceSlug`: a link to it, or a crumb
 * labelled with its slug. A link to a Project is never the service crumb,
 * even when the Project's name equals the service's slug.
 */
export function isServiceCrumb(
  item: BreadcrumbItem,
  serviceSlug: string
): boolean {
  if (item.href?.startsWith(`${PROJECT_GROUPS_PATH}/`)) return false
  return item.label === serviceSlug || item.href === `/projects/${serviceSlug}`
}

/**
 * The crumb naming `group`: a link to it, anywhere. On the group's own pages
 * (`onGroupPage`, from `resolveSidebarMode(pathname).kind === 'projectGroup'`)
 * also the unlinked crumb carrying its name, which is how those pages name
 * the current page; elsewhere a crumb that merely shares the name (a service
 * sub-page called "Settings" in a Project called "Settings") is not it.
 */
export function isProjectGroupCrumb(
  item: BreadcrumbItem,
  group: CrumbGroup,
  onGroupPage: boolean
): boolean {
  if (item.href === projectGroupHref(group.slug)) return true
  return onGroupPage && item.href === undefined && item.label === group.name
}

/**
 * Puts the service's Project right before the service crumb:
 * `Projects › <service>` becomes `Projects › <project> › <service>`. Returns
 * `trail` itself when there is nothing to add — no group, no service crumb,
 * or a trail that already links the group.
 */
export function withProjectGroupCrumb(
  trail: BreadcrumbItem[],
  serviceSlug: string | null,
  group: CrumbGroup | undefined
): BreadcrumbItem[] {
  if (!serviceSlug || !group) return trail
  const href = projectGroupHref(group.slug)
  if (trail.some((item) => item.href === href)) return trail
  const index = trail.findIndex((item) => isServiceCrumb(item, serviceSlug))
  if (index === -1) return trail
  return [
    ...trail.slice(0, index),
    { label: group.name, href },
    ...trail.slice(index),
  ]
}
