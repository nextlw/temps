// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import type { ProjectResponse } from '@/api/client'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { useRemoveServiceFromProjectGroup } from '@/hooks/useProjectGroups'
import { projectGroupErrorReason } from '@/lib/project-group-errors'
import { servicesOfGroup } from '@/lib/project-group-overview'
import type { ProjectGroupResponse } from '@/lib/project-groups-types'
import {
  Callout,
  DataTable,
  RecordLink,
  type DataTableColumn,
} from '@temps-sdk/ds'
import { Boxes, Plus, RefreshCw, Unlink } from 'lucide-react'
import { useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { AddServiceToProjectGroupDialog } from './AddServiceToProjectGroupDialog'
import { returnFocusTo } from './return-focus'
import {
  ServiceEnvironmentsCell,
  ServiceLastDeploymentCell,
} from './ServiceRuntimeSummary'

interface Props {
  group: ProjectGroupResponse
  groups: readonly ProjectGroupResponse[]
  catalog: readonly ProjectResponse[]
  catalogLoading: boolean
  catalogError: boolean
  onRetryCatalog: () => void
}

/** A Project's overview: its services, their environments and deployments. */
export function ProjectGroupOverview({
  group,
  groups,
  catalog,
  catalogLoading,
  catalogError,
  onRetryCatalog,
}: Props) {
  const { t } = useTranslation('projectGroups')
  const [adding, setAdding] = useState(false)
  const [removing, setRemoving] = useState<ProjectResponse | null>(null)
  // Focus goes back to the button that opened a dialog; after a removal that
  // row is gone, so it lands on the header's "Add service" instead.
  const addOpener = useRef<HTMLElement | null>(null)
  const removeOpener = useRef<HTMLElement | null>(null)
  const headerAdd = useRef<HTMLButtonElement | null>(null)
  const remove = useRemoveServiceFromProjectGroup()
  const services = useMemo(
    () => servicesOfGroup(group, catalog),
    [catalog, group]
  )

  const columns: DataTableColumn<ProjectResponse>[] = [
    {
      key: 'service',
      header: t('detail.columnService'),
      className: 'min-w-44',
      render: (service) => (
        <RecordLink to={`/projects/${service.slug}`}>{service.name}</RecordLink>
      ),
    },
    {
      key: 'environments',
      header: t('detail.columnEnvironments'),
      className: 'min-w-56',
      render: (service) => <ServiceEnvironmentsCell serviceId={service.id} />,
    },
    {
      key: 'lastDeployment',
      header: t('detail.columnLastDeployment'),
      className: 'min-w-40',
      render: (service) => <ServiceLastDeploymentCell serviceId={service.id} />,
    },
    {
      key: 'actions',
      header: <span className="sr-only">{t('detail.columnActions')}</span>,
      className: 'w-12 text-right',
      render: (service) => (
        <Button
          variant="ghost"
          size="icon"
          aria-label={t('detail.removeService', { name: service.name })}
          title={t('detail.removeService', { name: service.name })}
          onClick={(event) => {
            removeOpener.current = event.currentTarget
            remove.reset()
            setRemoving(service)
          }}
        >
          <Unlink className="size-4" />
        </Button>
      ),
    },
  ]

  const confirmRemove = () => {
    if (!removing) return
    const service = removing
    remove.mutate(
      { groupId: group.id, serviceId: service.id },
      {
        onSuccess: () => {
          toast.success(
            t('remove.removed', { service: service.name, name: group.name })
          )
          setRemoving(null)
        },
      }
    )
  }

  const openAdd = (event: React.MouseEvent<HTMLElement>) => {
    addOpener.current = event.currentTarget
    setAdding(true)
  }
  const addButton = (
    <Button size="sm" onClick={openAdd}>
      <Plus className="size-4" />
      {t('detail.addService')}
    </Button>
  )

  return (
    <section aria-labelledby="project-group-services" className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <h2 id="project-group-services" className="text-lg font-semibold">
            {t('detail.servicesHeading')}
          </h2>
          <p className="text-sm text-muted-foreground">
            {t('detail.servicesCount', { count: group.service_count })}
          </p>
        </div>
        <Button ref={headerAdd} size="sm" onClick={openAdd}>
          <Plus className="size-4" />
          {t('detail.addService')}
        </Button>
      </div>

      {catalogError && catalog.length === 0 ? (
        <div className="rounded-lg border bg-card text-card-foreground">
          <EmptyState
            size="compact"
            icon={Boxes}
            title={t('detail.servicesLoadFailed')}
            description={t('notFound.loadFailedDescription')}
            action={
              <Button variant="outline" size="sm" onClick={onRetryCatalog}>
                <RefreshCw className="size-4" />
                {t('detail.retry')}
              </Button>
            }
          />
        </div>
      ) : group.service_count === 0 ? (
        <div className="rounded-lg border bg-card text-card-foreground">
          <EmptyState
            size="compact"
            icon={Boxes}
            title={t('detail.noServicesTitle')}
            description={t('detail.noServicesDescription')}
            action={addButton}
          />
        </div>
      ) : (
        <DataTable
          aria-label={t('detail.servicesTable', { name: group.name })}
          columns={columns}
          rows={services}
          rowKey={(service) => service.id}
          isLoading={catalogLoading && catalog.length === 0}
        />
      )}

      <AddServiceToProjectGroupDialog
        open={adding}
        onOpenChange={setAdding}
        group={group}
        groups={groups}
        catalog={catalog}
        catalogLoading={catalogLoading && catalog.length === 0}
        catalogError={catalogError}
        opener={addOpener}
      />

      <AlertDialog
        open={removing !== null}
        onOpenChange={(open) => {
          if (!open) setRemoving(null)
        }}
      >
        <AlertDialogContent
          onCloseAutoFocus={(event) =>
            returnFocusTo(event, removeOpener, headerAdd)
          }
        >
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t('remove.title', {
                service: removing?.name ?? '',
                name: group.name,
              })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t('remove.description')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {remove.isError && (
            <Callout tone="error">
              {t(`errors.${projectGroupErrorReason(remove.error)}`)}
            </Callout>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={remove.isPending}>
              {t('remove.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={(event) => {
                // Stay open until the request settles, to show a failure.
                event.preventDefault()
                confirmRemove()
              }}
              disabled={remove.isPending}
            >
              {remove.isPending ? t('remove.removing') : t('remove.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  )
}
