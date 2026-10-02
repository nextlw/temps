// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import type { ProjectResponse } from '@/api/client'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { SettingsSection } from '@/components/ui/settings-section'
import {
  useAssignServiceToProjectGroup,
  useProjectGroups,
  useRemoveServiceFromProjectGroup,
} from '@/hooks/useProjectGroups'
import { projectGroupErrorReason } from '@/lib/project-group-errors'
import { groupOfService, projectGroupHref } from '@/lib/project-groups'
import { Callout } from '@temps-sdk/ds'
import { FolderKanban } from 'lucide-react'
import { useId } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { toast } from 'sonner'

const NONE = 'none'

/**
 * "Project" section of a service's general settings: which Project (code:
 * `project_group`) the service is in. A choice applies at once, as a PUT
 * (which moves it out of any other Project) or a DELETE for "None"; the
 * shared groups query is invalidated, so the sidebar and the breadcrumb
 * follow without a reload.
 */
export function ProjectGroupMembershipSection({
  project,
}: {
  project: ProjectResponse
}) {
  const { t } = useTranslation('projectGroups')
  const id = useId()
  const groupsQuery = useProjectGroups()
  const { groups } = groupsQuery
  const assign = useAssignServiceToProjectGroup()
  const remove = useRemoveServiceFromProjectGroup()
  const current = groupOfService(groups, project.id)
  const pending = assign.isPending || remove.isPending
  const failure = assign.error ?? remove.error

  const choose = (value: string) => {
    assign.reset()
    remove.reset()
    if (value === NONE) {
      if (!current) return
      remove.mutate(
        { groupId: current.id, serviceId: project.id },
        {
          onSuccess: () =>
            toast.success(t('membership.removed', { service: project.name })),
        }
      )
      return
    }
    const next = groups.find((group) => String(group.id) === value)
    if (!next || next.id === current?.id) return
    assign.mutate(
      { groupId: next.id, serviceId: project.id },
      {
        onSuccess: () =>
          toast.success(
            t('membership.moved', { service: project.name, name: next.name })
          ),
      }
    )
  }

  return (
    <SettingsSection
      title={t('membership.title')}
      icon={FolderKanban}
      description={t('membership.description')}
      defaultOpen
    >
      <div className="space-y-3">
        {groupsQuery.isError && !groupsQuery.data ? (
          <Callout tone="error">
            <span>{t('membership.loadFailed')} </span>
            <Button
              variant="link"
              size="sm"
              className="h-auto p-0"
              onClick={() => void groupsQuery.refetch()}
            >
              {t('membership.retry')}
            </Button>
          </Callout>
        ) : groups.length === 0 && !groupsQuery.isLoading ? (
          <p className="text-sm text-muted-foreground">
            {t('membership.noGroups')}{' '}
            <Link to="/projects" className="text-foreground underline">
              {t('membership.createLink')}
            </Link>
          </p>
        ) : (
          <div className="space-y-2">
            <Label htmlFor={id}>{t('membership.label')}</Label>
            <div className="flex flex-wrap items-center gap-3">
              <Select
                value={current ? String(current.id) : NONE}
                onValueChange={choose}
                disabled={pending || groupsQuery.isLoading}
              >
                <SelectTrigger
                  id={id}
                  className="w-full max-w-sm"
                  aria-describedby={`${id}-hint`}
                  aria-busy={pending}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NONE}>{t('membership.none')}</SelectItem>
                  <SelectSeparator />
                  {groups.map((group) => (
                    <SelectItem key={group.id} value={String(group.id)}>
                      {group.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {pending ? (
                <span role="status" className="text-sm text-muted-foreground">
                  {t('membership.saving')}
                </span>
              ) : current ? (
                <Link
                  to={projectGroupHref(current.slug)}
                  className="text-sm underline"
                >
                  {t('membership.open', { name: current.name })}
                </Link>
              ) : null}
            </div>
            <p id={`${id}-hint`} className="text-xs text-muted-foreground">
              {t('membership.hint')}
            </p>
          </div>
        )}
        {failure && (
          <Callout tone="error">
            {t(`errors.${projectGroupErrorReason(failure)}`)}
          </Callout>
        )}
      </div>
    </SettingsSection>
  )
}
