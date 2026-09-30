// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import {
  ReactNode,
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useLocation } from 'react-router'
import {
  type BreadcrumbItem,
  BreadcrumbContext,
  createBreadcrumbOwnership,
} from './BreadcrumbContext-shared'

export function BreadcrumbProvider({ children }: { children: ReactNode }) {
  const [breadcrumbs, setTrail] = useState<BreadcrumbItem[]>([])
  const { pathname } = useLocation()
  // Layout effects all run before any passive effect, so pages and layouts
  // setting their trail from useEffect always see the URL being rendered.
  const pathRef = useRef(pathname)
  useLayoutEffect(() => {
    pathRef.current = pathname
  }, [pathname])
  const [ownership] = useState(() => createBreadcrumbOwnership(setTrail))

  const setBreadcrumbs = useCallback(
    (items: BreadcrumbItem[]) => ownership.setPage(items, pathRef.current),
    [ownership]
  )
  const setLayoutBreadcrumbs = useCallback(
    (items: BreadcrumbItem[]) => ownership.setLayout(items, pathRef.current),
    [ownership]
  )
  const value = useMemo(
    () => ({ breadcrumbs, setBreadcrumbs, setLayoutBreadcrumbs }),
    [breadcrumbs, setBreadcrumbs, setLayoutBreadcrumbs]
  )

  return (
    <BreadcrumbContext.Provider value={value}>
      {children}
    </BreadcrumbContext.Provider>
  )
}
