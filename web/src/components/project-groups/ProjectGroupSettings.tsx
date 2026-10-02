// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

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
import { Button, buttonVariants } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  useDeleteProjectGroup,
  useUpdateProjectGroup,
} from '@/hooks/useProjectGroups'
import {
  PROJECT_GROUP_NAME_MAX,
  projectGroupErrorReason,
  projectGroupNameProblem,
} from '@/lib/project-group-errors'
import type { ProjectGroupResponse } from '@/api/client'
import { Callout, Field, SettingsGroup } from '@temps-sdk/ds'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { returnFocusTo } from './return-focus'

/** Rename, describe or delete a Project. The slug is shown, never edited. */
export function ProjectGroupSettings({
  group,
}: {
  group: ProjectGroupResponse
}) {
  const { t } = useTranslation('projectGroups')
  const navigate = useNavigate()
  const update = useUpdateProjectGroup()
  const remove = useDeleteProjectGroup()
  const [name, setName] = useState(group.name)
  const [description, setDescription] = useState(group.description ?? '')
  const [submitted, setSubmitted] = useState(false)
  const [confirmingDelete, setConfirmingDelete] = useState(false)
  const deleteOpener = useRef<HTMLButtonElement | null>(null)

  // Keep what the user is typing across refetches of the same group; load
  // the other group's values when the route switches to another one.
  const syncedId = useRef(group.id)
  useEffect(() => {
    if (syncedId.current === group.id) return
    syncedId.current = group.id
    setName(group.name)
    setDescription(group.description ?? '')
    setSubmitted(false)
    update.reset()
  }, [group.id, group.name, group.description, update])

  const trimmedName = name.trim()
  const nameProblem = projectGroupNameProblem(name)
  const nameError =
    !submitted || !nameProblem
      ? undefined
      : nameProblem === 'required'
        ? t('settings.nameRequired')
        : t('settings.nameTooLong', { max: PROJECT_GROUP_NAME_MAX })
  const trimmedDescription = description.trim()
  const dirty =
    trimmedName !== group.name ||
    trimmedDescription !== (group.description ?? '')

  const save = (event: React.FormEvent) => {
    event.preventDefault()
    setSubmitted(true)
    if (nameProblem || !dirty) return
    update.mutate(
      {
        id: group.id,
        body: {
          ...(trimmedName !== group.name ? { name: trimmedName } : {}),
          // An empty string clears the description (ADR-049, PATCH).
          ...(trimmedDescription !== (group.description ?? '')
            ? { description: trimmedDescription }
            : {}),
        },
      },
      {
        onSuccess: (saved) => {
          setName(saved.name)
          setDescription(saved.description ?? '')
          setSubmitted(false)
          toast.success(t('settings.saved'))
        },
      }
    )
  }

  const confirmDelete = () => {
    remove.mutate(group.id, {
      onSuccess: () => {
        setConfirmingDelete(false)
        toast.success(t('settings.deleted', { name: group.name }))
        navigate('/projects')
      },
    })
  }

  return (
    // Short forms keep a readable width (ds RULES, "Progressive disclosure").
    <div className="w-full min-w-0 max-w-5xl space-y-10">
      <SettingsGroup
        title={t('settings.generalTitle')}
        description={t('settings.generalDescription')}
      >
        <form className="space-y-5" onSubmit={save} noValidate>
          <Field label={t('settings.nameLabel')} error={nameError}>
            {(props) => (
              <Input
                {...props}
                value={name}
                onChange={(event) => setName(event.target.value)}
                autoComplete="off"
              />
            )}
          </Field>
          <Field
            label={t('settings.descriptionLabel')}
            description={t('settings.descriptionHint')}
          >
            {(props) => (
              <Textarea
                {...props}
                value={description}
                onChange={(event) => setDescription(event.target.value)}
                rows={3}
              />
            )}
          </Field>
          <Field
            label={t('settings.slugLabel')}
            description={t('settings.slugHint')}
          >
            {(props) => (
              <Input
                {...props}
                value={group.slug}
                readOnly
                className="font-mono"
              />
            )}
          </Field>
          {update.isError && (
            <Callout tone="error">
              {t(`errors.${projectGroupErrorReason(update.error)}`)}
            </Callout>
          )}
          <Button
            type="submit"
            size="sm"
            disabled={!dirty || update.isPending}
            aria-busy={update.isPending}
          >
            {update.isPending ? t('settings.saving') : t('settings.save')}
          </Button>
        </form>
      </SettingsGroup>

      <SettingsGroup
        title={t('settings.deleteTitle')}
        description={t('settings.deleteDescription')}
      >
        <div>
          <Button
            ref={deleteOpener}
            variant="destructive"
            onClick={() => {
              remove.reset()
              setConfirmingDelete(true)
            }}
          >
            {t('settings.deleteButton')}
          </Button>
        </div>
      </SettingsGroup>

      <AlertDialog open={confirmingDelete} onOpenChange={setConfirmingDelete}>
        <AlertDialogContent
          onCloseAutoFocus={(event) => returnFocusTo(event, deleteOpener)}
        >
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t('settings.deleteConfirmTitle', { name: group.name })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t('settings.deleteConfirmDescription', {
                count: group.service_count,
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {remove.isError && (
            <Callout tone="error">
              {t(`errors.${projectGroupErrorReason(remove.error)}`)}
            </Callout>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={remove.isPending}>
              {t('settings.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={(event) => {
                event.preventDefault()
                confirmDelete()
              }}
              disabled={remove.isPending}
              className={buttonVariants({ variant: 'destructive' })}
            >
              {remove.isPending
                ? t('settings.deleting')
                : t('settings.deleteButton')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
