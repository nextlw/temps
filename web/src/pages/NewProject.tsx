// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { useEffect } from 'react'
import { useBreadcrumbs } from '@/contexts/BreadcrumbContext'
import { useTranslation } from 'react-i18next'
import { usePageTitle } from '@/hooks/usePageTitle'
import { GitImportClone } from '@/components/project/GitImportClone'
import { PageContainer } from '@/components/layout/PageContainer'

export function NewProject() {
  const { setBreadcrumbs } = useBreadcrumbs()
  const { t } = useTranslation('nav')
  const { t: tp } = useTranslation('projects')

  useEffect(() => {
    setBreadcrumbs([
      { label: t('projects'), href: '/projects' },
      { label: tp('create.newProject') },
    ])
  }, [setBreadcrumbs, t, tp])

  usePageTitle(tp('create.newProject'))

  return (
    <PageContainer>
      <GitImportClone mode="navigation" />
    </PageContainer>
  )
}
