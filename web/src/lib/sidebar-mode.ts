// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { WORKER_NODES_URL } from '@/lib/worker-nodes'

// The sidebar is a pure function of the URL. Which nav it shows is derived
// from the pathname alone -- no component state, no history key -- so a
// reload, a shared link, and the browser's back/forward buttons all show the
// same sidebar as the page. Leaving a contextual nav is a real navigation to
// `SIDEBAR_BACK_TARGET[kind]`, never a local toggle.
export type SidebarMode =
  | { kind: 'default' }
  | { kind: 'settings' }
  | { kind: 'ai' }
  | { kind: 'project'; slug: string }

// The AI area's several pages read as one drill-down instead of scattered
// sidebar entries, mirroring the settings swap.
export const AI_MODE_PREFIXES = [
  '/ai-gateway',
  '/chat',
  '/agent-sandbox',
  '/skills',
  '/mcp-servers',
  '/ai-workflows',
] as const

// `/projects/<segment>` routes that are not a project.
const NON_PROJECT_SEGMENTS = new Set(['new', 'import-wizard', 'import'])

export const SIDEBAR_BACK_TARGET = {
  project: '/projects',
  // `/` only redirects to the project list; link there directly.
  settings: '/projects',
  ai: '/tools',
} as const satisfies Record<Exclude<SidebarMode['kind'], 'default'>, string>

// Prefixes match whole path segments: `/chat` covers `/chat` and `/chat/…`,
// not `/chatbots`.
const isUnder = (pathname: string, prefix: string) =>
  pathname === prefix || pathname.startsWith(`${prefix}/`)

const decodeSegment = (segment: string) => {
  try {
    return decodeURIComponent(segment)
  } catch {
    // Malformed escape (e.g. a lone `%`): keep the raw segment.
    return segment
  }
}

export function resolveSidebarMode(pathname: string): SidebarMode {
  // Worker Nodes keeps its historical /settings/nodes URL but is a main-nav
  // page ("Build & deliver"); swapping to the settings sidebar there would
  // hide the entry that is currently active.
  if (isUnder(pathname, '/settings') && !isUnder(pathname, WORKER_NODES_URL)) {
    return { kind: 'settings' }
  }
  if (AI_MODE_PREFIXES.some((prefix) => isUnder(pathname, prefix))) {
    return { kind: 'ai' }
  }
  const projectMatch = pathname.match(/^\/projects\/([^/]+)(?:\/.*)?$/)
  if (projectMatch && !NON_PROJECT_SEGMENTS.has(projectMatch[1])) {
    return { kind: 'project', slug: decodeSegment(projectMatch[1]) }
  }
  return { kind: 'default' }
}
