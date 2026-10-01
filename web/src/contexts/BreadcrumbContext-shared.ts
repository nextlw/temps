// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { createContext, useContext } from 'react'

export type BreadcrumbItem = {
  label: string
  href?: string
}

export type BreadcrumbContextType = {
  breadcrumbs: BreadcrumbItem[]
  /** Sets the trail of the current URL. The page that calls it owns it. */
  setBreadcrumbs: (items: BreadcrumbItem[]) => void
  /**
   * Sets a layout's default trail (e.g. the project shell's). Ignored while a
   * page rendered inside the layout owns the trail of the current URL.
   */
  setLayoutBreadcrumbs: (items: BreadcrumbItem[]) => void
}

export const BreadcrumbContext = createContext<
  BreadcrumbContextType | undefined
>(undefined)

export function useBreadcrumbs() {
  const context = useContext(BreadcrumbContext)
  if (context === undefined) {
    throw new Error('useBreadcrumbs must be used within a BreadcrumbProvider')
  }
  return context
}

/**
 * One owner per trail (DESIGN.md, "Shared breadcrumbs"). A page sets the trail
 * of its URL and so claims that URL; a layout's trail only applies to URLs no
 * page has claimed. React runs a child's effects before its parent's, so
 * without the claim a layout re-running its effect (on mount, or when its own
 * data loads) would overwrite the trail of the page nested inside it.
 */
export function createBreadcrumbOwnership(
  apply: (items: BreadcrumbItem[]) => void
) {
  let ownerPath: string | null = null
  return {
    setPage(items: BreadcrumbItem[], path: string) {
      ownerPath = path
      apply(items)
    },
    setLayout(items: BreadcrumbItem[], path: string) {
      if (ownerPath === path) return
      apply(items)
    },
  }
}
