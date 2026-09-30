// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// UI vocabulary for the console's resources — see
// docs/adr/048-ui-terminology-projects-services.md.
//
// Only the interface is renamed. Code, API, CLI, plugin SDK and AI tools keep
// `project` (and the new grouping entity is `project_group`); the words users
// read come from the `terms` namespace instead. So `terms.project` is the
// code entity `project`, whatever the UI calls it, and changing what it is
// called means changing the value in `locales/<lng>/terms.json`, never a key.
//
// Other namespaces reference these values with i18next nesting
// (`$t(terms:project.plural)`), so a rename lands everywhere at once.

import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

export interface EntityTerm {
  singular: string
  plural: string
  singularLower: string
  pluralLower: string
}

export interface Terms {
  /** Code entity `project` (a deployable unit). */
  project: EntityTerm
  /** Code entity `project_group` (the grouping above `project`). */
  projectGroup: EntityTerm
  /** External services (`/external-services`): managed databases/caches. */
  externalServices: string
  /** Built-in KV/Blob resources under `/projects/:slug/services/*`. */
  platformResources: string
  /** Docker Compose `service_name` entries of a project. */
  containers: string
  /** OpenTelemetry `service.name` attribute. */
  otelService: string
}

export function useTerms(): Terms {
  const { t } = useTranslation('terms')
  // `t` changes identity when the language changes, so this recomputes then
  // and otherwise hands consumers a stable object.
  return useMemo(() => {
    const entity = (key: 'project' | 'projectGroup'): EntityTerm => ({
      singular: t(`${key}.singular`),
      plural: t(`${key}.plural`),
      singularLower: t(`${key}.singularLower`),
      pluralLower: t(`${key}.pluralLower`),
    })
    return {
      project: entity('project'),
      projectGroup: entity('projectGroup'),
      externalServices: t('externalServices'),
      platformResources: t('platformResources'),
      containers: t('containers'),
      otelService: t('otelService'),
    }
  }, [t])
}
