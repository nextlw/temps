// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { ProjectAvatar } from '@/components/project/ProjectAvatar'
import { useProjectGroups } from '@/hooks/useProjectGroups'
import { projectGroupHref } from '@/lib/project-groups'
import { Check, ChevronsUpDown, Folder } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from '../ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '../ui/popover'

/**
 * The Project crumb of the header breadcrumb (UI: Project, code:
 * `project_group`): shows the current Project's name and switches to another
 * one, like the service crumb's switcher next to it.
 */
export function ProjectGroupSwitcher({
  currentSlug,
  label,
}: {
  currentSlug: string
  label: string
}) {
  const navigate = useNavigate()
  const { t } = useTranslation('projectGroups')
  const [open, setOpen] = useState(false)
  // Already cached by the sidebar and the breadcrumb; groups come by name.
  const { groups } = useProjectGroups()

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={t('switcher.switch')}
          className="inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-md px-1.5 py-0.5 text-sm font-normal text-foreground transition-colors hover:bg-accent"
        >
          <span className="max-w-[120px] truncate sm:max-w-[200px] lg:max-w-[280px]">
            {label}
          </span>
          <ChevronsUpDown className="size-3.5 shrink-0 text-muted-foreground" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        className="w-[280px] p-0"
        align="start"
        side="bottom"
        sideOffset={6}
      >
        <Command>
          <CommandInput placeholder={t('switcher.find')} />
          <CommandList>
            <CommandEmpty>{t('switcher.empty')}</CommandEmpty>
            <CommandGroup>
              {groups.map((group) => {
                const isCurrent = group.slug === currentSlug
                return (
                  <CommandItem
                    key={group.id}
                    value={`${group.name} ${group.slug}`}
                    onSelect={() => {
                      setOpen(false)
                      if (!isCurrent) navigate(projectGroupHref(group.slug))
                    }}
                  >
                    <ProjectAvatar
                      name={group.name}
                      className="size-5 rounded-sm"
                      fallbackClassName="rounded-sm bg-muted text-[10px] text-muted-foreground"
                    />
                    <span className="flex-1 truncate">{group.name}</span>
                    {isCurrent && (
                      <Check className="size-4 text-muted-foreground" />
                    )}
                  </CommandItem>
                )
              })}
            </CommandGroup>
            <CommandSeparator />
            <CommandGroup>
              <CommandItem
                onSelect={() => {
                  setOpen(false)
                  navigate('/projects')
                }}
              >
                <Folder className="size-4" />
                <span>{t('switcher.all')}</span>
              </CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}
