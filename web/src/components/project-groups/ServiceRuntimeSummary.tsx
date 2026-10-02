// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// The runtime columns of a Project's services table: each environment's state
// and the service's last deployment. One query per service, so a slow or
// failing service never blanks the others.

import { getLastDeployment } from '@/api/client'
import {
  getEnvironmentsOptions,
  getLastDeploymentQueryKey,
} from '@/api/client/@tanstack/react-query.gen'
import { Skeleton } from '@/components/ui/skeleton'
import { TimeAgo } from '@/components/utils/TimeAgo'
import {
  DEPLOYMENT_OUTCOME_TONE,
  ENVIRONMENT_STATE_TONE,
  deploymentOutcome,
  environmentState,
} from '@/lib/project-group-overview'
import { Status } from '@temps-sdk/ds'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

export function ServiceEnvironmentsCell({ serviceId }: { serviceId: number }) {
  const { t } = useTranslation('projectGroups')
  const { data, isLoading, isError } = useQuery({
    ...getEnvironmentsOptions({ path: { project_id: serviceId } }),
    staleTime: 30_000,
  })
  if (isLoading) return <Skeleton className="h-4 w-28" />
  if (isError || !data) {
    return (
      <span className="text-sm text-muted-foreground">
        {t('environments.unavailable')}
      </span>
    )
  }
  const environments = data.filter((environment) => !environment.is_preview)
  const previews = data.length - environments.length
  if (environments.length === 0 && previews === 0) {
    return (
      <span className="text-sm text-muted-foreground">
        {t('environments.none')}
      </span>
    )
  }
  return (
    <ul className="flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1">
      {environments.map((environment) => {
        const state = environmentState(environment)
        return (
          <li key={environment.id} className="flex min-w-0 items-center gap-2">
            <span className="truncate text-sm">{environment.name}</span>
            <Status
              variant="dot"
              tone={ENVIRONMENT_STATE_TONE[state]}
              label={t(`environments.${state}`)}
              className="text-muted-foreground"
            />
          </li>
        )
      })}
      {previews > 0 && (
        <li className="text-xs text-muted-foreground">
          {t('environments.previews', { count: previews })}
        </li>
      )}
    </ul>
  )
}

export function ServiceLastDeploymentCell({
  serviceId,
}: {
  serviceId: number
}) {
  const { t } = useTranslation('projectGroups')
  const { data, isLoading, isError } = useQuery({
    // The generated key, shared with the service pages. 404 is how the
    // endpoint says "never deployed", so here it reads as no deployment (the
    // service pages only check the value for truthiness).
    queryKey: getLastDeploymentQueryKey({ path: { id: serviceId } }),
    queryFn: async ({ signal }) => {
      const { data, error, response } = await getLastDeployment({
        path: { id: serviceId },
        signal,
      })
      if (response?.status === 404) return null
      if (error !== undefined || !data) throw error ?? new Error('empty')
      return data
    },
    staleTime: 30_000,
    retry: false,
  })
  if (isLoading) return <Skeleton className="h-4 w-24" />
  if (isError) {
    return (
      <span className="text-sm text-muted-foreground">
        {t('lastDeployment.unavailable')}
      </span>
    )
  }
  if (!data) {
    return (
      <span className="text-sm text-muted-foreground">
        {t('lastDeployment.none')}
      </span>
    )
  }
  const outcome = deploymentOutcome(data.status)
  return (
    <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
      <Status
        variant="dot"
        tone={DEPLOYMENT_OUTCOME_TONE[outcome]}
        label={t(`lastDeployment.${outcome}`, { status: data.status })}
      />
      <TimeAgo
        date={data.created_at}
        className="text-xs text-muted-foreground"
      />
    </span>
  )
}
