// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// A Project (code: `project_group`, docs/adr/049-project-groups.md) at
// `/project-groups/:groupSlug/*`: its overview (the services in it) and its
// settings. The page owns the trail of both; the header turns the Project's
// crumb into the Project switcher.

import { PageContainer, PageHeader } from '@/components/layout/PageContainer'
import { ProjectGroupOverview } from '@/components/project-groups/ProjectGroupOverview'
import { ProjectGroupSettings } from '@/components/project-groups/ProjectGroupSettings'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { Skeleton } from '@/components/ui/skeleton'
import { useBreadcrumbs } from '@/contexts/BreadcrumbContext'
import { usePageTitle } from '@/hooks/usePageTitle'
import { useProjectGroups } from '@/hooks/useProjectGroups'
import { useServiceCatalog } from '@/hooks/useServiceCatalog'
import { findProjectGroupBySlug, projectGroupHref } from '@/lib/project-groups'
import { resolveProjectGroupSection } from '@/lib/sidebar-mode'
import { FolderSearch, RefreshCw } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, Route, Routes, useLocation, useParams } from 'react-router'

export function ProjectGroupDetail() {
  const { groupSlug = '' } = useParams<{ groupSlug: string }>()
  const location = useLocation()
  const { t } = useTranslation(['projectGroups', 'nav'])
  const { setBreadcrumbs } = useBreadcrumbs()
  const groupsQuery = useProjectGroups()
  const { groups } = groupsQuery
  const group = findProjectGroupBySlug(groups, groupSlug)
  const section = resolveProjectGroupSection(location.pathname)
  const catalog = useServiceCatalog({ enabled: !!group })

  const waiting = groupsQuery.isLoading
  const failed = groupsQuery.isError && !groupsQuery.data
  const missing = !waiting && !failed && !group

  const groupName = group?.name
  useEffect(() => {
    const root = { label: t('nav:projects'), href: '/projects' }
    if (!groupName) {
      setBreadcrumbs([
        root,
        {
          label: waiting
            ? t('projectGroups:detail.loading')
            : t('projectGroups:notFound.crumb'),
        },
      ])
      return
    }
    setBreadcrumbs(
      section === 'settings'
        ? [
            root,
            { label: groupName, href: projectGroupHref(groupSlug) },
            { label: t('projectGroups:detail.settingsCrumb') },
          ]
        : [root, { label: groupName }]
    )
  }, [groupName, groupSlug, section, setBreadcrumbs, t, waiting])

  usePageTitle(
    groupName
      ? section === 'settings'
        ? `${t('projectGroups:detail.settingsCrumb')} - ${groupName}`
        : groupName
      : t('nav:projects')
  )

  if (waiting) {
    return (
      <PageContainer innerClassName="space-y-6">
        <div className="space-y-2" aria-busy="true">
          <Skeleton className="h-8 w-48" />
          <Skeleton className="h-4 w-72" />
        </div>
        <p role="status" className="sr-only">
          {t('projectGroups:detail.loading')}
        </p>
      </PageContainer>
    )
  }

  if (failed) {
    return (
      <PageContainer innerClassName="space-y-6">
        <PageHeader title={t('nav:projects')} />
        <div className="rounded-lg border bg-card text-card-foreground">
          <EmptyState
            size="compact"
            icon={FolderSearch}
            title={t('projectGroups:notFound.loadFailedTitle')}
            description={t('projectGroups:notFound.loadFailedDescription')}
            action={
              <Button
                variant="outline"
                size="sm"
                onClick={() => void groupsQuery.refetch()}
              >
                <RefreshCw className="size-4" />
                {t('projectGroups:notFound.retry')}
              </Button>
            }
          />
        </div>
      </PageContainer>
    )
  }

  if (missing || !group) {
    return (
      <NotFoundState
        title={t('projectGroups:notFound.title')}
        description={t('projectGroups:notFound.description', {
          slug: groupSlug,
        })}
      />
    )
  }

  return (
    <PageContainer innerClassName="space-y-6">
      <PageHeader
        title={group.name}
        description={group.description || undefined}
      />
      <Routes>
        <Route
          index
          element={
            <ProjectGroupOverview
              group={group}
              groups={groups}
              catalog={catalog.services}
              catalogLoading={catalog.isLoading}
              catalogError={catalog.isError}
              onRetryCatalog={() => void catalog.refetch()}
            />
          }
        />
        <Route
          path="settings"
          element={<ProjectGroupSettings group={group} />}
        />
        <Route
          path="*"
          element={
            <NotFoundSurface
              title={t('projectGroups:notFound.pageTitle')}
              description={t('projectGroups:notFound.pageDescription')}
              backTo={projectGroupHref(group.slug)}
              backLabel={group.name}
            />
          }
        />
      </Routes>
    </PageContainer>
  )
}

function NotFoundState({
  title,
  description,
}: {
  title: string
  description: string
}) {
  const { t } = useTranslation(['projectGroups', 'nav'])
  return (
    <PageContainer innerClassName="space-y-6">
      <PageHeader title={t('nav:projects')} />
      <NotFoundSurface
        title={title}
        description={description}
        backTo="/projects"
        backLabel={t('projectGroups:notFound.back')}
      />
    </PageContainer>
  )
}

function NotFoundSurface({
  title,
  description,
  backTo,
  backLabel,
}: {
  title: string
  description: string
  backTo: string
  backLabel: string
}) {
  return (
    <div className="rounded-lg border bg-card text-card-foreground">
      <EmptyState
        size="compact"
        icon={FolderSearch}
        title={title}
        description={description}
        action={
          <Button asChild variant="outline" size="sm">
            <Link to={backTo}>{backLabel}</Link>
          </Button>
        }
      />
    </div>
  )
}

export default ProjectGroupDetail
