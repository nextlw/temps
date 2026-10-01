// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { Button } from '@/components/ui/button'
import { FolderKanban } from 'lucide-react'
import { useTranslation } from 'react-i18next'

/**
 * Shown on `/projects` while no Project exists: the list stays today's list
 * of services, with this one entry point to group them (ADR-049, DF2-2).
 */
export function CreateProjectGroupCallout({
  onCreate,
}: {
  onCreate: () => void
}) {
  const { t } = useTranslation('projectGroups')
  return (
    <section
      aria-labelledby="create-project-group-cta"
      className="flex flex-col gap-3 rounded-lg border bg-card p-4 text-card-foreground sm:flex-row sm:items-center"
    >
      <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-muted">
        <FolderKanban
          className="size-4 text-muted-foreground"
          aria-hidden="true"
        />
      </span>
      <div className="min-w-0 flex-1">
        <h2 id="create-project-group-cta" className="text-sm font-medium">
          {t('list.ctaTitle')}
        </h2>
        <p className="text-sm text-muted-foreground">
          {t('list.ctaDescription')}
        </p>
      </div>
      <Button variant="outline" size="sm" onClick={onCreate}>
        {t('list.ctaAction')}
      </Button>
    </section>
  )
}
