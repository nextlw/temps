// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { useCreateProjectGroup } from '@/hooks/useProjectGroups'
import {
  PROJECT_GROUP_NAME_MAX,
  projectGroupErrorReason,
  projectGroupNameProblem,
} from '@/lib/project-group-errors'
import { projectGroupHref } from '@/lib/project-groups'
import { Callout, Field } from '@temps-sdk/ds'
import { useState, type RefObject } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { returnFocusTo } from './return-focus'

/**
 * Creates a Project (code: `project_group`) and opens it. The server derives
 * the slug from the name (ADR-049) and answers 409 when it is taken.
 */
export function CreateProjectGroupDialog({
  open,
  onOpenChange,
  opener,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The button that opened the dialog; focus returns to it on close. */
  opener: RefObject<HTMLElement | null>
}) {
  const { t } = useTranslation('projectGroups')
  const navigate = useNavigate()
  const create = useCreateProjectGroup()
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [submitted, setSubmitted] = useState(false)

  const trimmedName = name.trim()
  const nameProblem = projectGroupNameProblem(name)
  const nameError =
    !submitted || !nameProblem
      ? undefined
      : nameProblem === 'required'
        ? t('settings.nameRequired')
        : t('settings.nameTooLong', { max: PROJECT_GROUP_NAME_MAX })

  const close = (next: boolean) => {
    if (!next) {
      setName('')
      setDescription('')
      setSubmitted(false)
      create.reset()
    }
    onOpenChange(next)
  }

  const submit = (event: React.FormEvent) => {
    event.preventDefault()
    setSubmitted(true)
    if (nameProblem) return
    create.mutate(
      // Only contract fields, and only those with a value: the server
      // rejects unknown fields (400) and an absent description is unset.
      {
        name: trimmedName,
        ...(description.trim() ? { description: description.trim() } : {}),
      },
      {
        onSuccess: (group) => {
          toast.success(t('create.created', { name: group.name }))
          close(false)
          navigate(projectGroupHref(group.slug))
        },
      }
    )
  }

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent onCloseAutoFocus={(event) => returnFocusTo(event, opener)}>
        <form onSubmit={submit} noValidate className="space-y-4">
          <DialogHeader>
            <DialogTitle>{t('create.title')}</DialogTitle>
            <DialogDescription>{t('create.description')}</DialogDescription>
          </DialogHeader>
          <Field
            label={t('create.nameLabel')}
            description={t('create.nameHint')}
            error={nameError}
          >
            {(props) => (
              <Input
                {...props}
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={t('create.namePlaceholder')}
                autoComplete="off"
                autoFocus
              />
            )}
          </Field>
          <Field
            label={t('create.descriptionLabel')}
            description={t('create.descriptionHint')}
          >
            {(props) => (
              <Textarea
                {...props}
                value={description}
                onChange={(event) => setDescription(event.target.value)}
                rows={2}
              />
            )}
          </Field>
          {create.isError && (
            <Callout tone="error">
              {t(`errors.${projectGroupErrorReason(create.error)}`)}
            </Callout>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => close(false)}
            >
              {t('create.cancel')}
            </Button>
            <Button
              type="submit"
              disabled={create.isPending}
              aria-busy={create.isPending}
            >
              {create.isPending ? t('create.submitting') : t('create.submit')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
