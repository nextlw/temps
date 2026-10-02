// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import type { ProjectGroupResponse, ProjectResponse } from '@/api/client'
import { projectGroupHref } from '@/lib/project-groups'
import { ProjectAvatar, RecordLink } from '@temps-sdk/ds'
import { Box } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'

/** At most this many services are named on a card; the rest are counted. */
const SHOWN_SERVICES = 5

/**
 * A Project on the `/projects` list: its name (the link to its page), its
 * description, and the services in it, each a link to that service.
 */
export function ProjectGroupCard({
  group,
  services,
}: {
  group: ProjectGroupResponse
  /** The services to list: all of the group's, or the search's matches. */
  services: readonly ProjectResponse[]
}) {
  const { t } = useTranslation('projectGroups')
  const shown = services.slice(0, SHOWN_SERVICES)
  const hidden = Math.max(0, group.service_count - shown.length)
  const href = projectGroupHref(group.slug)

  return (
    <article className="flex min-w-0 flex-col gap-3 rounded-lg border bg-card p-4 text-card-foreground">
      <header className="flex min-w-0 items-start gap-3">
        <ProjectAvatar
          name={group.name}
          className="size-8 shrink-0 rounded-md"
          fallbackClassName="rounded-md bg-muted text-xs text-muted-foreground"
        />
        <div className="min-w-0 flex-1">
          <h3 className="min-w-0">
            <RecordLink to={href} className="min-h-0">
              {group.name}
            </RecordLink>
          </h3>
          <p className="text-xs text-muted-foreground">
            {t('detail.servicesCount', { count: group.service_count })}
          </p>
        </div>
      </header>
      {group.description && (
        <p className="line-clamp-2 text-sm text-muted-foreground">
          {group.description}
        </p>
      )}
      {group.service_count === 0 ? (
        <p className="text-sm text-muted-foreground">{t('card.noServices')}</p>
      ) : (
        <ul
          className="space-y-1"
          aria-label={t('card.servicesLabel', { name: group.name })}
        >
          {shown.map((service) => (
            <li key={service.id} className="min-w-0">
              <Link
                to={`/projects/${service.slug}`}
                className="flex min-w-0 items-center gap-2 rounded-sm text-sm hover:underline focus-visible:outline-2 focus-visible:outline-ring"
              >
                <Box
                  className="size-3.5 shrink-0 text-muted-foreground"
                  aria-hidden="true"
                />
                <span className="truncate">{service.name}</span>
              </Link>
            </li>
          ))}
          {hidden > 0 && (
            <li>
              <Link
                to={href}
                className="text-xs text-muted-foreground hover:underline"
              >
                {t('card.more', { count: hidden })}
              </Link>
            </li>
          )}
        </ul>
      )}
    </article>
  )
}
