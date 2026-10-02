// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useBreadcrumbs } from '@/contexts/BreadcrumbContext'
import { useTranslation } from 'react-i18next'
import { useDashboardAnalytics } from '@/hooks/useDashboardAnalytics'
import { useDashboardHealth } from '@/hooks/useDashboardHealth'
import { useProjectsMonitorHealth } from '@/hooks/useProjectsMonitorHealth'
import { useLatestDeploymentMedia } from '@/hooks/useLatestDeploymentMedia'
import { usePageTitle } from '@/hooks/usePageTitle'
import { useProjectGroups } from '@/hooks/useProjectGroups'
import { useServiceCatalog } from '@/hooks/useServiceCatalog'
import { CreateProjectGroupCallout } from '@/components/project-groups/CreateProjectGroupCallout'
import { CreateProjectGroupDialog } from '@/components/project-groups/CreateProjectGroupDialog'
import { GroupedProjects } from '@/components/project-groups/GroupedProjects'
import { groupedProjectsView } from '@/lib/project-groups-list'
import { Callout } from '@temps-sdk/ds'
import { FirstProjectOnboarding } from '@/components/dashboard/FirstProjectOnboarding'
import { SIMULATE_EMPTY_INSTALL } from '@/lib/devSimulate'
import { ProjectCard } from '@/components/dashboard/ProjectCard'
import { OnboardingNextStepCard } from '@/components/dashboard/OnboardingNextStepCard'
import { WorkerNodeRequiredBanner } from '@/components/nodes/WorkerNodeRequiredBanner'
import { ProjectCardSkeleton } from '@/components/skeletons/ProjectCardSkeleton'
import { Button } from '@/components/ui/button'
import { CreateActionButton } from '@/components/ui/create-action-button'
import { ListToolbar } from '@/components/layout/ListToolbar'
import { Alert, AlertTitle, AlertDescription } from '@/components/ui/alert'
import { ResponsivePagination } from '@/components/ui/responsive-pagination'
import { PageContainer, PageHeader } from '@/components/layout/PageContainer'
import {
  getProjectsOptions,
  listGitProvidersOptions,
} from '@/api/client/@tanstack/react-query.gen'
import { useQuery } from '@tanstack/react-query'
import { subDays } from 'date-fns'
import { ArrowRight, RefreshCw } from 'lucide-react'
import { Link, useSearchParams } from 'react-router'
import { SourceLogo } from '@/components/imports/SourceLogo'
import {
  TOP_MIGRATION_SOURCES,
  importHref,
} from '@/components/imports/migration-sources'
import {
  PROJECT_PAGE_SIZE_OPTIONS,
  projectPageCount,
  readProjectPagination,
  withProjectPagination,
  withProjectSearch,
} from '@/lib/project-list-pagination'

const SEARCH_CATALOG_LIMIT = 50

export function Projects() {
  const { setBreadcrumbs } = useBreadcrumbs()
  const { t } = useTranslation('nav')
  const { t: tp } = useTranslation('projects')
  const { t: tg } = useTranslation('projectGroups')
  const [searchParams, setSearchParams] = useSearchParams()
  const { page, pageSize } = readProjectPagination(searchParams)
  const projectSearch = searchParams.get('q') ?? ''
  const setProjectSearch = (value: string) => {
    setSearchParams((current) => withProjectSearch(current, value), {
      replace: true,
    })
  }
  const normalizedProjectSearch = projectSearch.trim().toLowerCase()

  // Projects (code: project groups, ADR-049): once one exists the list puts
  // the services under them, and pages only the ungrouped ones; until then
  // it stays the list of services it was.
  const projectGroups = useProjectGroups()
  const grouped = projectGroups.groups.length > 0
  const catalog = useServiceCatalog({ enabled: grouped })
  const [creatingGroup, setCreatingGroup] = useState(false)
  // Two buttons open the create dialog; focus returns to the one used.
  const createOpener = useRef<HTMLElement | null>(null)
  const openCreateGroup = (trigger: HTMLElement) => {
    createOpener.current = trigger
    setCreatingGroup(true)
  }

  const {
    data: rawProjectsData,
    isLoading,
    isError,
    isFetching,
    refetch,
  } = useQuery({
    ...getProjectsOptions({
      query: {
        page: normalizedProjectSearch ? 1 : page,
        // The list endpoint has no text filter, so search a deliberately
        // bounded catalogue, with the scope disclosed beside the filter.
        per_page: normalizedProjectSearch ? SEARCH_CATALOG_LIMIT : pageSize,
      },
    }),
    // With Projects the list reads the whole catalogue instead; this page is
    // only asked for once it is known there are none.
    enabled: !projectGroups.isLoading && !grouped,
  })

  const { data: rawGitProviders, isLoading: gitProvidersLoading } = useQuery({
    ...listGitProvidersOptions({}),
    retry: false,
  })

  // TEMP: force an empty (brand-new install) dashboard while iterating on the
  // first-run experience. See lib/devSimulate.ts.
  const projectsData = SIMULATE_EMPTY_INSTALL
    ? ({ ...rawProjectsData, projects: [], total: 0 } as typeof rawProjectsData)
    : rawProjectsData
  const gitProviders = SIMULATE_EMPTY_INSTALL ? [] : rawGitProviders
  const groupedView = useMemo(
    () =>
      grouped
        ? groupedProjectsView(
            projectGroups.groups,
            catalog.services,
            normalizedProjectSearch,
            page,
            pageSize
          )
        : null,
    [
      catalog.services,
      grouped,
      normalizedProjectSearch,
      page,
      pageSize,
      projectGroups.groups,
    ]
  )
  const listTotal = groupedView
    ? groupedView.ungroupedTotal
    : (projectsData?.total ?? 0)
  const totalPages = projectPageCount(listTotal, pageSize)
  const isPageOutOfRange =
    !normalizedProjectSearch && listTotal > 0 && page > totalPages

  const setPagination = useCallback(
    (nextPage: number, nextPageSize = pageSize, replace = false) => {
      setSearchParams(
        (current) =>
          withProjectPagination(current, {
            page: nextPage,
            pageSize: nextPageSize,
          }),
        { replace }
      )
    },
    [pageSize, setSearchParams]
  )

  useEffect(() => {
    const hasCanonicalPage = searchParams.get('page') === String(page)
    const hasCanonicalPageSize =
      searchParams.get('page_size') === String(pageSize)

    if (!hasCanonicalPage || !hasCanonicalPageSize) {
      setPagination(page, pageSize, true)
    }
  }, [page, pageSize, searchParams, setPagination])

  useEffect(() => {
    if (isPageOutOfRange) setPagination(totalPages, pageSize, true)
  }, [isPageOutOfRange, pageSize, setPagination, totalPages])

  const visibleProjects = useMemo(() => {
    const projects = projectsData?.projects ?? []
    if (!normalizedProjectSearch) return projects
    return projects.filter(
      (project) =>
        project.name.toLowerCase().includes(normalizedProjectSearch) ||
        project.slug.toLowerCase().includes(normalizedProjectSearch)
    )
  }, [normalizedProjectSearch, projectsData?.projects])

  // The cards below: the matching services, or the page of ungrouped ones.
  const listedProjects = groupedView
    ? groupedView.ungroupedPage
    : visibleProjects

  useEffect(() => {
    setBreadcrumbs([{ label: t('projects') }])
  }, [setBreadcrumbs, t])

  usePageTitle(t('projects'))

  // Batch fetch analytics for all visible projects
  const { startDate, endDate } = useMemo(() => {
    return {
      startDate: subDays(new Date(), 1).toISOString(),
      endDate: new Date().toISOString(),
    }
  }, [])

  const projectIds = useMemo(
    () => listedProjects.map((project) => project.id),
    [listedProjects]
  )

  const dashboardAnalytics = useDashboardAnalytics(
    projectIds,
    startDate,
    endDate
  )

  const dashboardHealth = useDashboardHealth(projectIds, startDate, endDate)
  // Uptime monitors answer for projects that simply had no visitors, which
  // traffic-derived health cannot. See project-card-health.ts.
  const monitorHealth = useProjectsMonitorHealth(projectIds)
  const latestDeploymentMedia = useLatestDeploymentMedia(projectIds)

  const renderProjectCards = () =>
    listedProjects.map((project) => (
      <ProjectCard
        key={project.id}
        project={project}
        layout="compact"
        analytics={dashboardAnalytics.data?.projects?.[String(project.id)]}
        analyticsLoading={dashboardAnalytics.isLoading}
        analyticsError={dashboardAnalytics.isError}
        healthLoading={dashboardHealth.isLoading}
        healthError={dashboardHealth.isError}
        health={dashboardHealth.data?.projects?.[String(project.id)]}
        monitorHealth={monitorHealth.data?.projects?.[String(project.id)]}
        latestDeploymentMedia={
          latestDeploymentMedia.data?.projects?.[String(project.id)]
        }
        latestDeploymentMediaLoading={latestDeploymentMedia.isLoading}
        latestDeploymentMediaError={
          latestDeploymentMedia.isError && !latestDeploymentMedia.data
        }
      />
    ))

  const servicesSummary = !projectsData
    ? isError
      ? tp('list.countUnavailable')
      : tp('list.loading')
    : normalizedProjectSearch
      ? tp('list.matching', {
          count: visibleProjects.length,
          loaded: projectsData.projects.length,
          total: projectsData.total,
        })
      : tp('list.total', { count: projectsData.total })
  // With no Project yet, the call to action is the one way to create one,
  // so the header does not offer a second, differently worded button.
  const showCreateCallout =
    !grouped &&
    !projectGroups.isLoading &&
    !projectGroups.isError &&
    (projectsData?.total ?? 0) > 0
  const groupedSummary = !groupedView
    ? undefined
    : !catalog.data
      ? tp('list.loading')
      : tg('list.summary', {
          projects: tg('list.projectsCount', {
            count: groupedView.groups.length,
          }),
          services: tp('list.total', {
            count: normalizedProjectSearch
              ? groupedView.groups.reduce(
                  (sum, group) => sum + group.services.length,
                  groupedView.ungroupedTotal
                )
              : catalog.services.length,
          }),
        })

  return (
    <PageContainer innerClassName="space-y-6">
      {/* Header */}
      <ProjectsHeader
        grouped={grouped}
        actions={
          <>
            <PlatformStrip />
            {!showCreateCallout && (
              <Button
                variant="outline"
                onClick={(event) => openCreateGroup(event.currentTarget)}
              >
                {tg('list.newProject')}
              </Button>
            )}
            <CreateActionButton
              to="/projects/new"
              label={tp('list.newProject')}
            />
          </>
        }
      />

      {/* Nothing here can be built or deployed until something can run it. */}
      <WorkerNodeRequiredBanner />

      {(projectsData?.total ?? catalog.data?.length ?? 0) > 0 && (
        <OnboardingNextStepCard />
      )}

      {projectGroups.isError && (
        <Callout tone="warning">
          <span>{tg('list.groupsFailed')} </span>
          <Button
            variant="link"
            size="sm"
            className="h-auto p-0"
            onClick={() => void projectGroups.refetch()}
          >
            {tg('list.retry')}
          </Button>
        </Callout>
      )}

      {showCreateCallout && (
        <CreateProjectGroupCallout onCreate={openCreateGroup} />
      )}

      {((projectsData?.total ?? 0) > 0 ||
        Boolean(projectSearch) ||
        grouped) && (
        <ListToolbar
          searchLabel={
            grouped ? tg('list.searchLabel') : tp('list.searchLabel')
          }
          placeholder={
            grouped
              ? tg('list.searchPlaceholder')
              : tp('list.searchPlaceholder')
          }
          value={projectSearch}
          onChange={setProjectSearch}
          summary={groupedSummary ?? servicesSummary}
        />
      )}

      {isError && (
        <Alert variant="destructive">
          <AlertTitle>{tp('list.loadErrorTitle')}</AlertTitle>
          <AlertDescription>
            <p>
              {projectsData
                ? tp('list.loadErrorStale')
                : tp('list.loadErrorHint')}
            </p>
            <Button
              variant="outline"
              size="sm"
              className="mt-3"
              disabled={isFetching}
              onClick={() => void refetch()}
            >
              <RefreshCw
                className={isFetching ? 'size-4 animate-spin' : 'size-4'}
              />
              {isFetching ? 'Retrying…' : tp('list.retry')}
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {isLoading ||
      projectGroups.isLoading ||
      (projectsData?.total === 0 && gitProvidersLoading) ||
      isPageOutOfRange ? (
        <div
          className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3"
          aria-label={tp('list.loadingLabel')}
          aria-busy="true"
        >
          {Array.from({ length: pageSize }).map((_, i) => (
            <ProjectCardSkeleton key={i} layout="compact" />
          ))}
        </div>
      ) : groupedView ? (
        <GroupedProjects
          groups={groupedView.groups}
          ungroupedTotal={groupedView.ungroupedTotal}
          ungroupedCards={renderProjectCards()}
          loading={catalog.isLoading}
          failed={catalog.isError && !catalog.data}
          onRetry={() => void catalog.refetch()}
          query={projectSearch.trim()}
          onClearQuery={() => setProjectSearch('')}
        />
      ) : isError && !projectsData ? null : projectsData?.total === 0 ? (
        // First-run onboarding. The component is context-aware: when a Git
        // provider is already connected it routes straight into the import
        // wizard (skipping the connect step), and it always surfaces the
        // "deploy a project with a database" and CLI paths.
        <FirstProjectOnboarding
          gitConnected={!!gitProviders && gitProviders.length > 0}
        />
      ) : visibleProjects.length === 0 ? (
        <div className="rounded-xl border border-dashed px-6 py-12 text-center">
          <p className="font-medium">{tp('list.noMatchTitle')}</p>
          <p className="mt-1 text-sm text-muted-foreground">
            {tp('list.noMatchHint', { query: projectSearch.trim() })}
          </p>
          <Button
            variant="outline"
            size="sm"
            className="mt-4"
            onClick={() => setProjectSearch('')}
          >
            Clear filter
          </Button>
        </div>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {renderProjectCards()}
        </div>
      )}

      {/* Pagination - Only show if there are projects */}
      {(groupedView ? catalog.data : projectsData) &&
        listTotal > 0 &&
        !normalizedProjectSearch &&
        !isPageOutOfRange && (
          <ResponsivePagination
            page={page}
            pageSize={pageSize}
            total={listTotal}
            totalPages={totalPages}
            pageSizeOptions={PROJECT_PAGE_SIZE_OPTIONS}
            ariaLabel={tp('list.paginationLabel')}
            pageSizeAriaLabel={tp('list.perPage')}
            className="pt-2"
            onPageChange={(nextPage) => setPagination(nextPage)}
            onPageSizeChange={(nextPageSize) => setPagination(1, nextPageSize)}
          />
        )}

      <CreateProjectGroupDialog
        open={creatingGroup}
        onOpenChange={setCreatingGroup}
        opener={createOpener}
      />
    </PageContainer>
  )
}

/**
 * Projects page header. The title block is fixed; `actions` is what the
 * migration-entry-point variants swap out.
 */
function ProjectsHeader({
  actions,
  grouped,
}: {
  actions: React.ReactNode
  grouped: boolean
}) {
  const { t } = useTranslation(['nav', 'projects', 'projectGroups'])
  // "Projects", as the sidebar and the breadcrumb call this page, with or
  // without Projects in it (ADR-049, DF2-2).
  return (
    <PageHeader
      title={t('nav:projects')}
      description={
        grouped
          ? t('projectGroups:list.description')
          : t('projects:list.description')
      }
      actions={actions}
    />
  )
}

/**
 * Migration entry point. The platforms themselves are the affordance: brand
 * marks sit inline in the header so someone arriving from Coolify or Dokploy
 * recognises the path instead of reading for it, and each mark deep-links the
 * import wizard with that source already selected — skipping its first step.
 */
function PlatformStrip() {
  return (
    <div className="flex items-center gap-1 rounded-md border p-1">
      {/* The label is the first thing to go when the header wraps on mobile —
          the brand marks still carry the meaning, and every one of them has an
          accessible name. */}
      <span className="hidden px-1.5 text-xs text-muted-foreground sm:inline">
        Migrate from
      </span>
      {TOP_MIGRATION_SOURCES.map((p) => (
        <Link
          key={p.source}
          to={importHref(p.source)}
          title={`Import from ${p.label}`}
          aria-label={`Import from ${p.label}`}
          className="rounded p-1.5 transition-colors hover:bg-accent"
        >
          <SourceLogo source={p.source} className="h-4 w-4" />
        </Link>
      ))}
      <Link
        to="/projects/import-wizard"
        className="rounded p-1.5 text-muted-foreground transition-colors hover:bg-accent"
        title="All platforms"
        aria-label="All platforms"
      >
        <ArrowRight className="h-4 w-4" />
      </Link>
    </div>
  )
}
