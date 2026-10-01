// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { badgeVariants } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import { FolderKanban } from 'lucide-react'
import { useTranslation } from 'react-i18next'

/**
 * The Project (code: `project_group`) a service belongs to, as a small
 * neutral badge. A `span`, so it can sit inside buttons, links and options.
 * Screen readers hear `label` ("Project: CRM" by default), sighted users see
 * the name next to a folder icon.
 */
export function ProjectGroupBadge({
  name,
  label,
  className,
}: {
  name: string
  label?: string
  className?: string
}) {
  const { t } = useTranslation('projectGroups')
  const spoken = label ?? t('badge.label', { name })
  return (
    <span
      className={cn(
        badgeVariants({ variant: 'outline' }),
        'max-w-40 shrink-0 gap-1 font-medium text-muted-foreground',
        className
      )}
      title={spoken}
    >
      <FolderKanban className="size-3 shrink-0" aria-hidden="true" />
      <span className="truncate" aria-hidden="true">
        {name}
      </span>
      <span className="sr-only">{spoken}</span>
    </span>
  )
}
