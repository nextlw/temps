// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { useTranslation } from 'react-i18next'
import { getProjectCloudTelemetryOptions } from '@/api/client/@tanstack/react-query.gen'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { cloudAiMetadataMissing } from '@/lib/cloud-ai-metadata'
import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'

export function CloudAiMetadataNotice({
  projectId,
  projectSlug,
}: {
  projectId: number
  projectSlug?: string
}) {
  const { t } = useTranslation('ai')
  const { data } = useQuery(
    getProjectCloudTelemetryOptions({ path: { project_id: projectId } })
  )
  if (!data || !cloudAiMetadataMissing(data)) return null
  return (
    <Alert>
      <AlertTitle>AI metadata is not fully enabled for Cloud</AlertTitle>
      <AlertDescription>
        {t('cloudMetadata.body')}{' '}
        {projectSlug && (
          <Link
            className="underline"
            to={`/projects/${projectSlug}/settings/telemetry`}
          >
            Configure AI metadata
          </Link>
        )}
      </AlertDescription>
    </Alert>
  )
}
