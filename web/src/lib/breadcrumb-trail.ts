// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Render-time additions to the shared breadcrumb trail. Pages own their trail
// (one owner per trail, DESIGN.md "Shared breadcrumbs") and keep building it
// as `Projects › <service> › …`. Which Project a service is in is data the
// pages do not load, so the header adds that crumb when it renders, without
// touching what the owner set.

import type { BreadcrumbItem } from '@/contexts/BreadcrumbContext-shared'
import { projectGroupHref } from '@/lib/project-groups'

export interface CrumbGroup {
  slug: string
  name: string
}

/** The crumb naming the service `serviceSlug` (by link, or by slug label). */
export function isServiceCrumb(
  item: BreadcrumbItem,
  serviceSlug: string
): boolean {
  return item.label === serviceSlug || item.href === `/projects/${serviceSlug}`
}

/**
 * The crumb naming `group`: a link to it, or the unlinked current page that
 * carries its name (the group's own pages).
 */
export function isProjectGroupCrumb(
  item: BreadcrumbItem,
  group: CrumbGroup
): boolean {
  return (
    item.href === projectGroupHref(group.slug) ||
    (item.href === undefined && item.label === group.name)
  )
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
