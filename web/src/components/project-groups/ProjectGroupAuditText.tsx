// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Render-time translation of the audit entries of the Projects (code:
// project groups): the audit module keeps keys, these components turn them
// into text, so nothing is translated at import time (F6.0).

import {
  describeProjectGroupAudit,
  type AuditKey,
} from '@/lib/project-group-audit'
import { useTranslation } from 'react-i18next'

export function AuditKeyLabel({ labelKey }: { labelKey: AuditKey }) {
  const { t } = useTranslation('audit')
  return <>{t(labelKey)}</>
}

export function ProjectGroupAuditDescription({
  operation,
  data,
}: {
  operation: string
  data?: Record<string, unknown>
}) {
  const { t } = useTranslation('audit')
  const description = describeProjectGroupAudit(operation, data)
  if (!description) return <>{operation}</>
  return <>{t(description.key, description.values)}</>
}
