// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// The two sections of `/projects` once Projects exist (ADR-049, DF2-2):
// Projects, each a card with its services, and the services in none.

import type { ProjectResponse } from '@/api/client'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { Skeleton } from '@/components/ui/skeleton'
import type { ServicesInGroup } from '@/lib/project-groups'
import { Boxes, FolderKanban, RefreshCw, SearchX } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { ProjectGroupCard } from './ProjectGroupCard'

interface Props {
  groups: ServicesInGroup<ProjectResponse>[]
  ungroupedTotal: number
  /** The ungrouped services of the current page, rendered by the list page. */
  ungroupedCards: ReactNode
  loading: boolean
  failed: boolean
  onRetry: () => void
  query: string
  onClearQuery: () => void
}

export function GroupedProjects({
  groups,
  ungroupedTotal,
  ungroupedCards,
  loading,
  failed,
  onRetry,
  query,
  onClearQuery,
}: Props) {
  const { t } = useTranslation('projectGroups')

  if (loading) {
    return (
      <div
        className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3"
        aria-busy="true"
        aria-label={t('detail.loading')}
      >
        {Array.from({ length: 3 }).map((_, i) => (
          <Skeleton key={i} className="h-40 rounded-lg" />
        ))}
      </div>
    )
  }

  if (failed) {
    return (
      <div className="rounded-lg border bg-card text-card-foreground">
        <EmptyState
          size="compact"
          icon={Boxes}
          title={t('list.loadFailedTitle')}
          description={t('list.loadFailedHint')}
          action={
            <Button variant="outline" size="sm" onClick={onRetry}>
              <RefreshCw className="size-4" />
              {t('list.retry')}
            </Button>
          }
        />
      </div>
    )
  }

  if (query && groups.length === 0 && ungroupedTotal === 0) {
    return (
      <div className="rounded-lg border bg-card text-card-foreground">
        <EmptyState
          size="compact"
          icon={SearchX}
          title={t('list.noMatchTitle')}
          description={t('list.noMatchHint', { query })}
          action={
            <Button variant="outline" size="sm" onClick={onClearQuery}>
              {t('list.clearFilter')}
            </Button>
          }
        />
      </div>
    )
  }

  return (
    <div className="space-y-8">
      {groups.length > 0 && (
        <section
          aria-labelledby="projects-groups-heading"
          className="space-y-4"
        >
          <div>
            <h2 id="projects-groups-heading" className="text-lg font-semibold">
              {t('list.projectsHeading')}
            </h2>
            <p className="text-sm text-muted-foreground">
              {t('list.projectsCount', { count: groups.length })}
            </p>
          </div>
          <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
            {groups.map(({ group, services }) => (
              <ProjectGroupCard
                key={group.id}
                group={group}
                services={services}
              />
            ))}
          </div>
        </section>
      )}

      {(ungroupedTotal > 0 || !query) && (
        <section
          aria-labelledby="projects-ungrouped-heading"
          className="space-y-4"
        >
          <div>
            <h2
              id="projects-ungrouped-heading"
              className="text-lg font-semibold"
            >
              {t('list.ungroupedHeading')}
            </h2>
            <p className="text-sm text-muted-foreground">
              {t('list.ungroupedDescription')}
            </p>
          </div>
          {ungroupedTotal === 0 ? (
            <div className="rounded-lg border bg-card text-card-foreground">
              <EmptyState
                size="compact"
                icon={FolderKanban}
                title={t('list.ungroupedEmpty')}
                description={t('list.ungroupedEmptyHint')}
              />
            </div>
          ) : (
            <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
              {ungroupedCards}
            </div>
          )}
        </section>
      )}
    </div>
  )
}
