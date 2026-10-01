// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import type { ProjectResponse } from '@/api/client'
import { Button } from '@/components/ui/button'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useAssignServiceToProjectGroup } from '@/hooks/useProjectGroups'
import { projectGroupErrorReason } from '@/lib/project-group-errors'
import { addableServices } from '@/lib/project-group-overview'
import type { ProjectGroupResponse } from '@/lib/project-groups-types'
import { Callout } from '@temps-sdk/ds'
import { ArrowRightLeft, Box, Check } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ProjectGroupBadge } from './ProjectGroupBadge'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  group: ProjectGroupResponse
  groups: readonly ProjectGroupResponse[]
  catalog: readonly ProjectResponse[]
  catalogError: boolean
}

/**
 * Picks a service to put in `group`. A service in another Project shows that
 * Project, and the confirmation says it moves from there (the API moves it
 * in one step, docs/adr/049-project-groups.md).
 */
export function AddServiceToProjectGroupDialog({
  open,
  onOpenChange,
  group,
  groups,
  catalog,
  catalogError,
}: Props) {
  const { t } = useTranslation('projectGroups')
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const assign = useAssignServiceToProjectGroup()
  const candidates = useMemo(
    () => addableServices(groups, catalog, group),
    [catalog, group, groups]
  )
  const selected = candidates.find((c) => c.service.id === selectedId)

  const close = (next: boolean) => {
    if (!next) {
      setSelectedId(null)
      assign.reset()
    }
    onOpenChange(next)
  }

  const submit = () => {
    if (!selected) return
    assign.mutate(
      { groupId: group.id, serviceId: selected.service.id },
      {
        onSuccess: () => {
          toast.success(
            t('add.added', { service: selected.service.name, to: group.name })
          )
          close(false)
        },
      }
    )
  }

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('add.title', { name: group.name })}</DialogTitle>
          <DialogDescription>{t('add.description')}</DialogDescription>
        </DialogHeader>

        {catalogError ? (
          <Callout tone="error">{t('add.loadFailed')}</Callout>
        ) : candidates.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {t('add.noneAvailable')}
          </p>
        ) : (
          <Command className="rounded-md border">
            <CommandInput placeholder={t('add.find')} />
            <CommandList className="max-h-72" aria-label={t('add.listLabel')}>
              <CommandEmpty>{t('add.empty')}</CommandEmpty>
              <CommandGroup>
                {candidates.map(({ service, currentGroup }) => {
                  const isSelected = service.id === selectedId
                  return (
                    <CommandItem
                      key={service.id}
                      value={`${service.name} ${service.slug}`}
                      onSelect={() => setSelectedId(service.id)}
                      className="gap-2"
                    >
                      <Box className="size-4 text-muted-foreground" />
                      <span className="min-w-0 flex-1 truncate">
                        {service.name}
                      </span>
                      {currentGroup ? (
                        <ProjectGroupBadge
                          name={currentGroup.name}
                          label={t('add.inGroup', { name: currentGroup.name })}
                        />
                      ) : (
                        <span className="text-xs text-muted-foreground">
                          {t('add.ungrouped')}
                        </span>
                      )}
                      <Check
                        aria-hidden="true"
                        className={
                          isSelected ? 'size-4 opacity-100' : 'size-4 opacity-0'
                        }
                      />
                    </CommandItem>
                  )
                })}
              </CommandGroup>
            </CommandList>
          </Command>
        )}

        <div aria-live="polite" className="space-y-2">
          {selected ? (
            <p className="flex items-start gap-2 text-sm">
              {selected.currentGroup && (
                <ArrowRightLeft
                  className="mt-0.5 size-4 shrink-0 text-muted-foreground"
                  aria-hidden="true"
                />
              )}
              <span>
                {selected.currentGroup
                  ? t('add.movesFrom', {
                      service: selected.service.name,
                      from: selected.currentGroup.name,
                      to: group.name,
                    })
                  : t('add.joins', {
                      service: selected.service.name,
                      to: group.name,
                    })}
              </span>
            </p>
          ) : candidates.length > 0 ? (
            <p className="text-sm text-muted-foreground">
              {t('add.pickFirst')}
            </p>
          ) : null}
          {assign.isError && (
            <Callout tone="error">
              {t(`errors.${projectGroupErrorReason(assign.error)}`)}
            </Callout>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => close(false)}>
            {t('add.cancel')}
          </Button>
          <Button
            onClick={submit}
            disabled={!selected || assign.isPending}
            aria-busy={assign.isPending}
          >
            {assign.isPending
              ? t('add.submitting')
              : selected?.currentGroup
                ? t('add.submitMove')
                : t('add.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
