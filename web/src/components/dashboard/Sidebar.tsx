// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import {
  useConsoleExtensions,
  type ConsoleNavItem,
} from '@temps-sdk/console-kit'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from '@/components/ui/sidebar'
import {
  Activity,
  ArrowLeft,
  BadgeCheck,
  BarChart3,
  Bot,
  Box,
  Boxes,
  ChevronsUpDown,
  Check,
  Database,
  DatabaseBackup,
  Folder,
  Cpu,
  Gauge,
  GitBranch,
  GitFork,
  Globe,
  Home,
  Layers,
  LogOut,
  Monitor,
  Moon,
  Network,
  Search,
  ScrollText,
  MessageSquare,
  Server,
  Settings,
  ShieldAlert,
  Sun,
  Sparkles,
  Terminal,
  Wand2,
  Variable,
} from 'lucide-react'

import {
  getProjectBySlugOptions,
  getProjectsOptions,
} from '@/api/client/@tanstack/react-query.gen'
import { useAuth } from '@/contexts/AuthContext'
import { useGettingStarted } from '@/hooks/useGettingStarted'
import { useProjectGroups } from '@/hooks/useProjectGroups'
import { usePluginsContext } from '@/contexts/PluginsContext'
import { isPlatformToolsRoute } from '@/lib/platform-navigation'
import { resolvePluginIcon } from '@/lib/pluginIcons'
import { resolveProjectPrimaryRoute } from '@/lib/project-navigation'
import {
  findProjectGroupBySlug,
  groupOfService,
  groupServices,
  projectGroupHref,
} from '@/lib/project-groups'
import { WORKER_NODES_URL } from '@/lib/worker-nodes'
import { cn } from '@/lib/utils'
import {
  SIDEBAR_BACK_TARGET,
  projectNavBackTarget,
  resolveProjectGroupSection,
  resolveSidebarMode,
} from '@/lib/sidebar-mode'
import { useQuery } from '@tanstack/react-query'
import type { ParseKeys } from 'i18next'
import { type LucideIcon } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useLocation } from 'react-router'
import { Avatar, AvatarFallback, AvatarImage } from '../ui/avatar'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '../ui/dropdown-menu'
import { useTheme } from 'next-themes'
import { FeatureMaturityBadge } from '@/components/feature-maturity/FeatureMaturityBadge'
import {
  mergeSettingsNavigationGroups,
  settingsNavigationGroups,
  type SettingsNavigationIcon,
} from '@/components/settings/settings-navigation'

type NavKey = ParseKeys<'nav'>

interface PlatformNavItem {
  titleKey: NavKey
  url: string
  icon: LucideIcon
  activeWhen?: (pathname: string) => boolean
  featureKey?: string
}

interface PlatformNavGroup {
  labelKey: NavKey
  items: PlatformNavItem[]
}

// The fresh-install sidebar stays focused on daily work, grouped by intent so
// the short list is easy to scan. Every secondary capability remains visible
// on /tools and in Cmd+K.
const primaryPlatformGroups: PlatformNavGroup[] = [
  {
    labelKey: 'groups.buildDeliver',
    items: [
      { titleKey: 'platform.aiWorkspace', url: '/ai-first', icon: Sparkles },
      { titleKey: 'projects', url: '/projects', icon: Folder },
      {
        titleKey: 'platform.gitProviders',
        url: '/git-providers',
        icon: GitBranch,
      },
      { titleKey: 'platform.domains', url: '/domains', icon: Globe },
      // Lives under the /settings/nodes URL for historical reasons, but it is
      // a build-and-deliver capability: without a worker node a control plane
      // that runs no local workloads cannot build or deploy anything. See
      // WORKER_NODES_URL below for why the sidebar does not treat it as a
      // settings route.
      {
        titleKey: 'platform.workerNodes',
        url: WORKER_NODES_URL,
        icon: Network,
        featureKey: 'multi-node-worker-join',
      },
    ],
  },
  {
    labelKey: 'groups.data',
    items: [
      { titleKey: 'platform.databases', url: '/storage', icon: Database },
      { titleKey: 'platform.backups', url: '/backups', icon: DatabaseBackup },
    ],
  },
  {
    labelKey: 'groups.observe',
    items: [
      { titleKey: 'platform.analytics', url: '/analytics', icon: BarChart3 },
      { titleKey: 'platform.traces', url: '/traces', icon: GitFork },
      { titleKey: 'platform.logs', url: '/logs', icon: ScrollText },
      { titleKey: 'platform.errors', url: '/errors', icon: ShieldAlert },
      {
        titleKey: 'platform.server',
        url: '/monitoring/server',
        icon: Cpu,
        activeWhen: (pathname) => pathname.startsWith('/monitoring/server'),
      },
      {
        titleKey: 'platform.monitoring',
        url: '/monitoring/alerts',
        icon: Gauge,
        activeWhen: (pathname) =>
          pathname.startsWith('/monitoring') &&
          !pathname.startsWith('/monitoring/server'),
        featureKey: 'alerts-metric-alerts',
      },
      { titleKey: 'platform.proxy', url: '/proxy', icon: Activity },
    ],
  },
  {
    labelKey: 'groups.more',
    items: [
      {
        titleKey: 'platform.allPlatformTools',
        url: '/tools',
        icon: Boxes,
        activeWhen: isPlatformToolsRoute,
      },
    ],
  },
]

// AI drill-down — swapped in on AI_MODE_PREFIXES (see lib/sidebar-mode), so
// AI's several pages read as one coherent area instead of a scattered set of
// sidebar entries.
const aiNavItems: PlatformNavItem[] = [
  {
    titleKey: 'ai.harnesses',
    url: '/agent-sandbox/providers',
    icon: Terminal,
    featureKey: 'ai-chat',
  },
  {
    titleKey: 'ai.providers',
    url: '/ai-gateway',
    icon: Sparkles,
    featureKey: 'ai-gateway',
  },
  {
    titleKey: 'ai.usage',
    url: '/ai-gateway/usage',
    icon: BarChart3,
    featureKey: 'ai-gateway',
  },
  {
    titleKey: 'ai.activity',
    url: '/ai-gateway/activity',
    icon: Activity,
    featureKey: 'ai-gateway',
  },
  {
    titleKey: 'ai.setup',
    url: '/ai-gateway/setup',
    icon: Terminal,
    featureKey: 'ai-gateway',
  },
  {
    titleKey: 'ai.chats',
    url: '/chat',
    icon: MessageSquare,
    featureKey: 'ai-chat',
  },
  {
    titleKey: 'ai.workflows',
    url: '/ai-workflows',
    icon: Bot,
    featureKey: 'ai-agents-workflows',
  },
  {
    titleKey: 'ai.skills',
    url: '/skills',
    icon: Wand2,
    featureKey: 'ai-foundation-api-tools',
  },
  { titleKey: 'ai.mcpServers', url: '/mcp-servers', icon: Server },
]

function NavPlugins({
  items,
}: {
  items: { title: string; url: string; icon: LucideIcon }[]
}) {
  const location = useLocation()
  const { isMinimal, isMobile } = useSidebar()
  const { t } = useTranslation('nav')

  if (items.length === 0) return null

  return (
    <SidebarGroup
      className={
        isMinimal && !isMobile ? '' : 'group-data-[collapsible=icon]:hidden'
      }
    >
      <SidebarGroupLabel className={isMinimal && !isMobile ? 'hidden' : ''}>
        {t('groups.plugins')}
      </SidebarGroupLabel>
      <SidebarMenu>
        {items.map((item) => {
          const isActive =
            location.pathname === item.url ||
            (location.pathname.startsWith(item.url + '/') &&
              !items.some(
                (other) =>
                  other.url !== item.url &&
                  other.url.startsWith(item.url + '/') &&
                  (location.pathname === other.url ||
                    location.pathname.startsWith(other.url + '/'))
              ))
          return (
            <SidebarMenuItem key={item.title}>
              <SidebarMenuButton
                asChild
                tooltip={isMinimal && !isMobile ? item.title : undefined}
                className={cn(
                  'justify-center',
                  (!isMinimal || isMobile) && 'justify-start',
                  isActive && 'bg-sidebar-accent text-sidebar-accent-foreground'
                )}
              >
                <Link to={item.url}>
                  <item.icon />
                  {(!isMinimal || isMobile) && <span>{item.title}</span>}
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
          )
        })}
      </SidebarMenu>
    </SidebarGroup>
  )
}

// Command palette trigger pinned at the top of the sidebar.
// Styled like Vercel's sidebar Find input: bordered, full-width, with a
// keyboard-hint badge on the right.
function NavCommandTrigger() {
  const { isMinimal, isMobile } = useSidebar()
  const { t } = useTranslation()
  const compact = isMinimal && !isMobile
  const triggerCommand = () => {
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'k', metaKey: true })
    )
  }
  if (compact) {
    return (
      <SidebarGroup className="pb-0">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              tooltip={t('findWithShortcut')}
              onClick={triggerCommand}
              className="justify-center text-muted-foreground hover:text-foreground"
            >
              <Search />
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarGroup>
    )
  }
  return (
    <SidebarGroup className="pb-0">
      <button
        type="button"
        onClick={triggerCommand}
        className="flex h-8 w-full items-center gap-2 rounded-md border border-sidebar-border bg-transparent px-2 text-sm text-muted-foreground transition-colors hover:border-sidebar-border/80 hover:bg-sidebar-accent/40 hover:text-foreground"
      >
        <Search className="size-4 shrink-0" />
        <span className="flex-1 text-left">{t('find')}</span>
        <kbd className="rounded border border-sidebar-border bg-sidebar/60 px-1.5 py-0.5 text-[10px] tabular-nums text-muted-foreground">
          ⌘K
        </kbd>
      </button>
    </SidebarGroup>
  )
}

export default function AppSidebar() {
  const { isMinimal, isMobile } = useSidebar()
  const { platformNavEntries } = usePluginsContext()
  const location = useLocation()
  const { logoBadge, logoText, logoIcon } = useConsoleExtensions()

  // Convert plugin nav entries to sidebar item format
  const pluginItems = useMemo(
    () =>
      platformNavEntries.map((entry) => ({
        title: entry.label,
        url: entry.path,
        icon: resolvePluginIcon(entry.icon),
      })),
    [platformNavEntries]
  )

  // Route-driven sidebar swap, derived from the URL alone (settings, AI,
  // /projects/:slug/*, or the default workspace nav). Each contextual nav
  // leaves through a real link, so page and sidebar never disagree.
  const mode = resolveSidebarMode(location.pathname)

  const compact = isMinimal && !isMobile
  const { t } = useTranslation()

  return (
    <Sidebar>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <Link
              to="/"
              className={cn(
                'flex items-center gap-2 rounded-md transition-colors hover:bg-sidebar-accent/40',
                compact && 'justify-center'
              )}
            >
              <div
                className={cn(
                  'flex aspect-square size-8 items-center justify-center rounded-lg',
                  compact && 'w-6 h-6'
                )}
              >
                {logoIcon ?? (
                  <img
                    src="/svg/temps-icon.svg"
                    alt={t('logo')}
                    className="size-full"
                  />
                )}
              </div>
              {!compact && (
                <div className="grid flex-1 text-left text-sm leading-tight">
                  <span className="flex items-center gap-1.5 truncate font-semibold">
                    {logoText ?? 'Temps'}
                    {logoBadge}
                  </span>
                  <span className="truncate text-xs">
                    {import.meta.env.TEMPS_VERSION}
                  </span>
                </div>
              )}
            </Link>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <NavCommandTrigger />
        <GettingStartedNavItem />
        {mode.kind === 'settings' ? (
          <SettingsNav />
        ) : mode.kind === 'ai' ? (
          <AiNav />
        ) : mode.kind === 'project' ? (
          <ProjectNav slug={mode.slug} />
        ) : mode.kind === 'projectGroup' ? (
          <ProjectGroupNav slug={mode.slug} />
        ) : (
          <DefaultNav pluginItems={pluginItems} />
        )}
      </SidebarContent>
      <SidebarFooter>
        <NavUser />
      </SidebarFooter>
    </Sidebar>
  )
}

/**
 * Reusable labeled nav section used by variants 2-4.
 * Mirrors NavObserve styling so it inherits hover/active states.
 */
function NavSection({
  label,
  items,
  siblingUrls,
}: {
  label: string
  items: {
    id?: string
    title: string
    url: string
    icon: SettingsNavigationIcon
    activeWhen?: (pathname: string) => boolean
    featureKey?: string
  }[]
  // URLs of items in OTHER sections that share the sidebar. Used so a
  // parent-like url (e.g. `/settings`) doesn't light up when a more
  // specific sibling (`/settings/keys`) in a different section matches.
  siblingUrls?: string[]
}) {
  const location = useLocation()
  const { isMinimal, isMobile } = useSidebar()
  const compact = isMinimal && !isMobile
  const allUrls = useMemo(
    () => [...items.map((i) => i.url), ...(siblingUrls ?? [])],
    [items, siblingUrls]
  )
  // Active = the single longest url (across this section + siblings)
  // that is either an exact match or a path-prefix of the current
  // pathname. Keeps only the most specific match highlighted.
  const activeUrl = useMemo(
    () =>
      allUrls
        .filter(
          (url) =>
            location.pathname === url || location.pathname.startsWith(url + '/')
        )
        .reduce<string | null>(
          (best, url) =>
            best === null || url.length > best.length ? url : best,
          null
        ),
    [allUrls, location.pathname]
  )
  return (
    <SidebarGroup
      className={compact ? '' : 'group-data-[collapsible=icon]:hidden'}
    >
      <SidebarGroupLabel className={compact ? 'hidden' : ''}>
        {label}
      </SidebarGroupLabel>
      <SidebarMenu>
        {items.map((item) => {
          const isActive =
            item.activeWhen?.(location.pathname) ?? item.url === activeUrl
          return (
            <SidebarMenuItem key={item.id ?? item.url}>
              <SidebarMenuButton
                asChild
                tooltip={compact ? item.title : undefined}
                className={cn(
                  compact ? 'justify-center' : 'justify-start',
                  isActive && 'bg-sidebar-accent text-sidebar-accent-foreground'
                )}
              >
                <Link to={item.url}>
                  <item.icon />
                  {!compact && <span>{item.title}</span>}
                  {!compact && (
                    <FeatureMaturityBadge
                      featureKey={item.featureKey}
                      compact
                    />
                  )}
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
          )
        })}
      </SidebarMenu>
    </SidebarGroup>
  )
}

// Persistent link to platform setup progress, pinned just below the Find
// (⌘K) box at the top of the sidebar content so it shows on every page
// regardless of which nav mode (default/settings/project) is active. Styled
// as a bordered callout card (not a plain nav row) with a mini progress bar
// so it reads as a distinct "you have setup left" prompt. Full checklist
// detail lives on its own /setup page. Renders nothing once dismissed or
// fully complete (same visibility rule as the /setup page).
function GettingStartedNavItem() {
  const { isMinimal, isMobile } = useSidebar()
  const compact = isMinimal && !isMobile
  const { completedCount, totalCount, visible } = useGettingStarted()

  if (!visible) return null

  const pct = Math.round((completedCount / totalCount) * 100)

  if (compact) {
    return (
      <SidebarGroup className="pb-0">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              asChild
              tooltip={`Platform setup — ${completedCount}/${totalCount}`}
              className="justify-center"
            >
              <Link to="/setup">
                <BadgeCheck />
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarGroup>
    )
  }

  return (
    <SidebarGroup className="pb-0">
      <Link
        to="/setup"
        className="group flex flex-col gap-2 rounded-lg border border-sidebar-border bg-sidebar-accent/40 px-3 py-2.5 transition-colors hover:border-primary/40 hover:bg-sidebar-accent/70"
      >
        <div className="flex items-center gap-2">
          <BadgeCheck className="size-4 shrink-0 text-primary" />
          <span className="flex-1 text-sm font-medium">Platform setup</span>
          <span className="text-xs tabular-nums text-muted-foreground">
            {completedCount}/{totalCount}
          </span>
        </div>
        <div className="h-1 w-full overflow-hidden rounded-full bg-sidebar-border">
          <div
            className="h-full rounded-full bg-primary transition-all duration-500"
            style={{ width: `${pct}%` }}
          />
        </div>
      </Link>
    </SidebarGroup>
  )
}

/** Light / Dark / System, nested under the account menu. */
function ThemeSubmenu() {
  const { theme, setTheme } = useTheme()
  const options = [
    { value: 'light', label: 'Light', icon: Sun },
    { value: 'dark', label: 'Dark', icon: Moon },
    { value: 'system', label: 'System', icon: Monitor },
  ] as const
  const current = options.find((o) => o.value === theme) ?? options[2]
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        <current.icon className="mr-2 h-4 w-4" />
        <span>Appearance</span>
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent>
        {options.map((o) => (
          <DropdownMenuItem key={o.value} onClick={() => setTheme(o.value)}>
            <o.icon className="mr-2 h-4 w-4" />
            <span>{o.label}</span>
            {theme === o.value && <Check className="ml-auto h-4 w-4" />}
          </DropdownMenuItem>
        ))}
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}

function NavUser() {
  const { user } = useAuth()
  const { isMobile, isMinimal, setOpenMobile } = useSidebar()
  const { logout } = useAuth()
  if (!user) return null

  // Mobile renders inside a Radix Sheet (Dialog) with z-[9999] on the
  // overlay. A nested DropdownMenu portals to body and inherits z-50,
  // so the menu pops up behind the sheet and is invisible/unclickable.
  // Skip the dropdown on mobile: tap the row → /account directly,
  // with Log out as a sibling icon button so it's still one tap.
  // The desktop dropdown is unchanged.
  if (isMobile) {
    return (
      <SidebarMenu>
        <SidebarMenuItem>
          <div className="flex items-center gap-1">
            <SidebarMenuButton
              size="lg"
              asChild
              className="flex-1"
              onClick={() => setOpenMobile(false)}
            >
              <Link to="/account" aria-label="Open account settings">
                <Avatar className="h-8 w-8 rounded-lg">
                  <AvatarImage
                    src={user.avatar_url || ''}
                    alt={user.username || ''}
                  />
                  <AvatarFallback className="rounded-lg">
                    {user.username?.slice(0, 2).toUpperCase() || 'U'}
                  </AvatarFallback>
                </Avatar>
                <div className="grid min-w-0 flex-1 text-left text-sm leading-tight">
                  <span className="truncate font-semibold">
                    {user.username || 'User'}
                  </span>
                  <span className="truncate text-xs">{user.email}</span>
                </div>
              </Link>
            </SidebarMenuButton>
            <button
              type="button"
              onClick={async () => {
                await logout()
              }}
              className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
              aria-label="Log out"
              title="Log out"
            >
              <LogOut className="h-4 w-4" />
            </button>
          </div>
        </SidebarMenuItem>
      </SidebarMenu>
    )
  }

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              size="lg"
              className="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
            >
              <Avatar className="h-8 w-8 rounded-lg">
                <AvatarImage
                  src={user.avatar_url || ''}
                  alt={user.username || ''}
                />
                <AvatarFallback className="rounded-lg">
                  {user.username?.slice(0, 2).toUpperCase() || 'U'}
                </AvatarFallback>
              </Avatar>
              {!isMinimal && (
                <div className="grid flex-1 text-left text-sm leading-tight">
                  <span className="truncate font-semibold">
                    {user.username || 'User'}
                  </span>
                  <span className="truncate text-xs">{user.email}</span>
                </div>
              )}
              <ChevronsUpDown className="ml-auto size-4" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            className="w-(--radix-dropdown-menu-trigger-width) min-w-56 rounded-lg"
            side="right"
            align="end"
            sideOffset={4}
          >
            <DropdownMenuLabel className="p-0 font-normal">
              <div className="flex items-center gap-2 px-1 py-1.5 text-left text-sm">
                <Avatar className="h-8 w-8 rounded-lg">
                  <AvatarImage
                    src={user.avatar_url || ''}
                    alt={user.username || ''}
                  />
                  <AvatarFallback className="rounded-lg">
                    {user.username?.slice(0, 2).toUpperCase() || 'U'}
                  </AvatarFallback>
                </Avatar>
                <div className="grid flex-1 text-left text-sm leading-tight">
                  <span className="truncate font-semibold">
                    {user.username || 'User'}
                  </span>
                  <span className="truncate text-xs">{user.email}</span>
                </div>
              </div>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />

            <DropdownMenuGroup>
              <DropdownMenuItem>
                <Link to="/account" className="flex items-center">
                  <BadgeCheck className="mr-2 h-4 w-4" />
                  <span>Account</span>
                </Link>
              </DropdownMenuItem>
              {/* Appearance lives with the account rather than as a fourth
                  icon in the header — it's a per-user preference you set once,
                  not something you reach for while working. */}
              <ThemeSubmenu />
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onClick={async () => {
                await logout()
              }}
            >
              <LogOut />
              Log out
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}

// ─────────────────────────────────────────────────────────────────────────────
// Default workspace nav (root /, /sandboxes, /proxy, plugins, …).
// ─────────────────────────────────────────────────────────────────────────────

interface NavProps {
  pluginItems: { title: string; url: string; icon: LucideIcon }[]
}

function ExtensionNav({ items }: { items?: ConsoleNavItem[] }) {
  const location = useLocation()
  const { isMinimal, isMobile } = useSidebar()
  const compact = isMinimal && !isMobile

  if (!items || items.length === 0) return null

  const sections: string[] = []
  const bySection = new Map<string, ConsoleNavItem[]>()
  for (const item of items) {
    const key = item.section ?? 'Enterprise'
    if (!bySection.has(key)) {
      bySection.set(key, [])
      sections.push(key)
    }
    bySection.get(key)!.push(item)
  }

  return (
    <>
      {sections.map((section) => (
        <SidebarGroup
          key={section}
          className={compact ? '' : 'group-data-[collapsible=icon]:hidden'}
        >
          <SidebarGroupLabel className={compact ? 'hidden' : ''}>
            {section}
          </SidebarGroupLabel>
          <SidebarMenu>
            {bySection.get(section)!.map((item) => {
              const isActive =
                location.pathname === item.path ||
                location.pathname.startsWith(item.path + '/')
              return (
                <SidebarMenuItem key={item.id}>
                  <SidebarMenuButton
                    asChild
                    tooltip={compact ? item.label : undefined}
                    className={cn(
                      'justify-center',
                      !compact && 'justify-start',
                      isActive &&
                        'bg-sidebar-accent text-sidebar-accent-foreground'
                    )}
                  >
                    <Link to={item.path}>
                      {item.icon}
                      {!compact && <span>{item.label}</span>}
                    </Link>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              )
            })}
          </SidebarMenu>
        </SidebarGroup>
      ))}
    </>
  )
}

function DefaultNav({ pluginItems }: NavProps) {
  const { isMinimal, isMobile } = useSidebar()
  const compact = isMinimal && !isMobile
  const { t } = useTranslation('nav')

  const { navItems: extraNavItems } = useConsoleExtensions()
  const platformUrls = useMemo(
    () =>
      primaryPlatformGroups.flatMap((group) =>
        group.items.map((item) => item.url)
      ),
    []
  )

  return (
    <>
      {primaryPlatformGroups.map((group) => (
        <NavSection
          key={group.labelKey}
          label={t(group.labelKey)}
          items={group.items.map((item) => ({
            ...item,
            title: t(item.titleKey),
          }))}
          siblingUrls={platformUrls.filter(
            (url) => !group.items.some((item) => item.url === url)
          )}
        />
      ))}
      <NavPlugins items={pluginItems} />
      <ExtensionNav items={extraNavItems} />
      <SidebarGroup className="mt-auto">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              asChild
              tooltip={compact ? t('platform.settingsTooltip') : undefined}
              className={compact ? 'justify-center' : 'justify-start'}
            >
              <Link to="/settings">
                <Settings />
                {!compact && <span>{t('platform.settings')}</span>}
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarGroup>
    </>
  )
}

// ─────────────────────────────────────────────────────────────────────────────
// Settings nav — replaces the whole sidebar when on /settings/*.
// Back links to the main menu.
// ─────────────────────────────────────────────────────────────────────────────

function SettingsNav() {
  const { t } = useTranslation('nav')
  // Extension-provided links (e.g. an identity-provider page from a
  // console extension) join the built-in groups here, so instance
  // configuration lands in Settings rather than the workspace nav.
  const { settingsNavItems: extensionSettingsItems } = useConsoleExtensions()
  const groups = useMemo(
    () =>
      mergeSettingsNavigationGroups(
        settingsNavigationGroups,
        extensionSettingsItems
      ),
    [extensionSettingsItems]
  )
  // Every url across every settings group. Each section gets the list
  // minus its own items so active-state resolution sees the full tree
  // (prevents `/settings` lighting up on `/settings/keys`).
  const allSettingsUrls = groups.flatMap((g) => g.items.map((i) => i.url))
  return (
    <>
      <SwapHeader
        title={t('settings.title')}
        backTo={SIDEBAR_BACK_TARGET.settings}
        backText={t('back.mainMenu')}
        backLabel={t('back.toMainMenu')}
      />
      {groups.map((group) => {
        const ownUrls = new Set(group.items.map((i) => i.url))
        const siblings = allSettingsUrls.filter((u) => !ownUrls.has(u))
        return (
          <NavSection
            key={group.label}
            label={group.label}
            items={group.items}
            siblingUrls={siblings}
          />
        )
      })}
    </>
  )
}

// ─────────────────────────────────────────────────────────────────────────────
// AI nav — replaces the whole sidebar for the AI area (Providers, Usage,
// Chats, Workflows, Skills, MCP Servers). Back links to the platform tools,
// where the AI area is listed.
// ─────────────────────────────────────────────────────────────────────────────

function AiNav() {
  const { t } = useTranslation('nav')
  const items = aiNavItems.map((item) => ({
    ...item,
    title: t(item.titleKey),
  }))
  return (
    <>
      <SwapHeader
        title={t('ai.title')}
        backTo={SIDEBAR_BACK_TARGET.ai}
        backText={t('back.platformTools')}
        backLabel={t('back.toPlatformTools')}
      />
      <NavSection label={t('ai.title')} items={items} />
    </>
  )
}

// ─────────────────────────────────────────────────────────────────────────────
// Project nav — replaces the whole sidebar when on /projects/:slug/*.
// Back links to the project list.
// ─────────────────────────────────────────────────────────────────────────────

interface ProjectNavItem {
  titleKey: NavKey
  tooltipKey?: NavKey
  url: string
  icon: LucideIcon
  section: string
}

const projectPrimaryItems: readonly ProjectNavItem[] = [
  {
    titleKey: 'project.overview',
    url: 'project',
    icon: Home,
    section: 'project',
  },
  {
    titleKey: 'project.deployments',
    url: 'deployments',
    icon: GitBranch,
    section: 'deployments',
  },
  {
    titleKey: 'project.environments',
    url: 'environments',
    icon: Layers,
    section: 'environments',
  },
  {
    titleKey: 'project.environmentVariables',
    url: 'environment-variables',
    icon: Variable,
    section: 'environment-variables',
  },
  {
    titleKey: 'project.logs',
    url: 'runtime',
    icon: ScrollText,
    section: 'logs',
  },
  {
    titleKey: 'project.errors',
    url: 'errors',
    icon: ShieldAlert,
    section: 'errors',
  },
  {
    titleKey: 'project.traces',
    url: 'traces',
    icon: GitFork,
    section: 'traces',
  },
  {
    titleKey: 'project.analytics',
    url: 'analytics',
    icon: BarChart3,
    section: 'analytics',
  },
  {
    titleKey: 'project.monitoring',
    url: 'metrics',
    icon: Activity,
    section: 'monitoring',
  },
  {
    titleKey: 'project.databases',
    url: 'storage',
    icon: Database,
    section: 'storage',
  },
  {
    titleKey: 'project.security',
    url: 'security',
    icon: ShieldAlert,
    section: 'security',
  },
  {
    titleKey: 'project.settings',
    tooltipKey: 'project.settingsTooltip',
    url: 'settings/general',
    icon: Settings,
    section: 'settings',
  },
]

function ProjectNav({ slug }: { slug: string }) {
  const { data: project } = useQuery(
    getProjectBySlugOptions({ path: { slug } })
  )
  const location = useLocation()
  const { isMinimal, isMobile, setOpenMobile } = useSidebar()
  const { t } = useTranslation(['nav', 'common'])
  const compact = isMinimal && !isMobile
  const active = resolveProjectPrimaryRoute(
    location.pathname.slice(`/projects/${slug}/`.length)
  )
  // The service's Project, if it is in one, is where its back link leads.
  const { groups } = useProjectGroups()
  const group = project ? groupOfService(groups, project.id) : undefined
  return (
    <>
      <SwapHeader
        title={project?.name ?? t('common:loading')}
        backTo={projectNavBackTarget(group)}
        backText={group?.name ?? t('projects')}
        backLabel={
          group
            ? t('back.toProjectGroup', { name: group.name })
            : t('back.toProjects')
        }
      />
      <SidebarGroup className="py-2">
        <SidebarMenu aria-label={t('project.navigationLabel')}>
          {projectPrimaryItems.map((item) => (
            <SidebarMenuItem key={item.section} data-tour={item.section}>
              <SidebarMenuButton
                asChild
                tooltip={
                  compact ? t(item.tooltipKey ?? item.titleKey) : undefined
                }
                className={cn(
                  compact ? 'justify-center' : 'justify-start',
                  active === item.section &&
                    'bg-sidebar-accent text-sidebar-accent-foreground'
                )}
              >
                <Link
                  to={`/projects/${slug}/${item.url}`}
                  aria-current={active === item.section ? 'page' : undefined}
                  onClick={() => isMobile && setOpenMobile(false)}
                >
                  <item.icon />
                  {!compact && <span>{t(item.titleKey)}</span>}
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
          ))}
        </SidebarMenu>
      </SidebarGroup>
    </>
  )
}

// ─────────────────────────────────────────────────────────────────────────────
// Project group nav (UI: Project) — replaces the whole sidebar when on
// /project-groups/:slug/*. Lists the group's services, each leading into its
// own service nav; back links to the Projects list.
// ─────────────────────────────────────────────────────────────────────────────

function ProjectGroupNav({ slug }: { slug: string }) {
  const { groups, isLoading } = useProjectGroups()
  const group = findProjectGroupBySlug(groups, slug)
  // Same entry as the header's service switcher.
  const { data: servicesPage } = useQuery({
    ...getProjectsOptions({ query: { page: 1, per_page: 100 } }),
    enabled: !!group && group.service_count > 0,
  })
  const location = useLocation()
  const { isMinimal, isMobile, setOpenMobile } = useSidebar()
  const { t } = useTranslation(['nav', 'common'])
  const compact = isMinimal && !isMobile
  const active = resolveProjectGroupSection(location.pathname)
  const href = projectGroupHref(slug)
  const items = [
    {
      section: 'overview',
      titleKey: 'projectGroup.overview',
      url: href,
      icon: Home,
    },
    {
      section: 'settings',
      titleKey: 'projectGroup.settings',
      tooltipKey: 'projectGroup.settingsTooltip',
      url: `${href}/settings`,
      icon: Settings,
    },
  ] as const
  const services = useMemo(() => {
    if (!group) return []
    const sorted = (servicesPage?.projects ?? [])
      .slice()
      .sort((a, b) =>
        a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
      )
    return groupServices([group], sorted).groups[0].services
  }, [group, servicesPage?.projects])
  return (
    <>
      <SwapHeader
        title={group?.name ?? (isLoading ? t('common:loading') : slug)}
        backTo={SIDEBAR_BACK_TARGET.projectGroup}
        backText={t('projects')}
        backLabel={t('back.toProjects')}
      />
      <SidebarGroup className="py-2">
        <SidebarMenu aria-label={t('projectGroup.navigationLabel')}>
          {items.map((item) => (
            <SidebarMenuItem key={item.section}>
              <SidebarMenuButton
                asChild
                tooltip={
                  compact
                    ? t('tooltipKey' in item ? item.tooltipKey : item.titleKey)
                    : undefined
                }
                className={cn(
                  compact ? 'justify-center' : 'justify-start',
                  active === item.section &&
                    'bg-sidebar-accent text-sidebar-accent-foreground'
                )}
              >
                <Link
                  to={item.url}
                  aria-current={active === item.section ? 'page' : undefined}
                  onClick={() => isMobile && setOpenMobile(false)}
                >
                  <item.icon />
                  {!compact && <span>{t(item.titleKey)}</span>}
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
          ))}
        </SidebarMenu>
      </SidebarGroup>
      {group && (
        <SidebarGroup className={compact ? '' : 'py-0'}>
          <SidebarGroupLabel className={compact ? 'hidden' : ''}>
            {t('projectGroup.services')}
          </SidebarGroupLabel>
          {services.length === 0 ? (
            // Nothing listed: "no services" only for a Project without
            // members; members off the loaded page are counted below.
            !compact &&
            group.service_count === 0 && (
              <p className="px-2 text-sm text-muted-foreground">
                {t('projectGroup.noServices')}
              </p>
            )
          ) : (
            <SidebarMenu aria-label={t('projectGroup.servicesLabel')}>
              {services.map((service) => (
                <SidebarMenuItem key={service.id}>
                  <SidebarMenuButton
                    asChild
                    tooltip={compact ? service.name : undefined}
                    className={compact ? 'justify-center' : 'justify-start'}
                  >
                    <Link
                      to={`/projects/${service.slug}`}
                      onClick={() => isMobile && setOpenMobile(false)}
                    >
                      <Box />
                      {!compact && (
                        <span className="truncate">{service.name}</span>
                      )}
                    </Link>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          )}
          {/* Members beyond the loaded page of services are counted, not
              listed; the Project's overview lists them all. */}
          {!compact &&
            servicesPage !== undefined &&
            group.service_count > services.length && (
              <p className="px-2 pt-1 text-xs text-muted-foreground">
                {t('projectGroup.moreServices', {
                  count: group.service_count - services.length,
                })}
              </p>
            )}
        </SidebarGroup>
      )}
    </>
  )
}

// Shared header of the contextual navs (Settings, AI, Project): a real link
// back to where the nav was entered from, then the name of the area you are
// in. The link names its destination (`backText`, e.g. "← Projects"), never
// the current area, so it cannot be mistaken for a local toggle; collapsed,
// only the arrow remains and `backLabel` ("Back to projects") becomes its
// tooltip and accessible name.
function SwapHeader({
  title,
  backTo,
  backText,
  backLabel,
}: {
  title: string
  backTo: string
  backText: string
  backLabel: string
}) {
  const { isMinimal, isMobile, setOpenMobile } = useSidebar()
  const compact = isMinimal && !isMobile
  const closeOnMobile = () => isMobile && setOpenMobile(false)
  if (compact) {
    return (
      <SidebarGroup className="pb-0">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              asChild
              tooltip={backLabel}
              className="justify-center text-muted-foreground hover:text-foreground"
            >
              <Link to={backTo} aria-label={backLabel}>
                <ArrowLeft />
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarGroup>
    )
  }
  return (
    <SidebarGroup className="gap-1 pb-0">
      <Link
        to={backTo}
        aria-label={backLabel}
        onClick={closeOnMobile}
        className="flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-sm text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-foreground"
      >
        <ArrowLeft className="size-4 shrink-0" />
        <span className="truncate">{backText}</span>
      </Link>
      <div className="truncate px-2 text-sm font-semibold text-foreground">
        {title}
      </div>
    </SidebarGroup>
  )
}
