// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { afterEach, describe, expect, it } from 'bun:test'
import { getI18n } from 'react-i18next'
import {
  DEFAULT_NAMESPACE,
  FALLBACK_LANGUAGE,
  NAMESPACES,
  SUPPORTED_LANGUAGES,
  i18n,
} from './index'

describe('i18n init', () => {
  afterEach(async () => {
    await i18n.changeLanguage(FALLBACK_LANGUAGE)
  })

  it('is initialised synchronously on import with the en catalogs', () => {
    expect(i18n.isInitialized).toBe(true)
    expect(i18n.language).toBe('en')
    expect(i18n.options.defaultNS).toBe(DEFAULT_NAMESPACE)
    for (const ns of NAMESPACES) {
      expect(i18n.hasResourceBundle('en', ns)).toBe(true)
    }
  })

  it('is not registered as the global react-i18next instance', () => {
    // Components get it from <I18nextProvider> in <TempsConsole>; a host
    // edition's own react-i18next setup must stay untouched.
    expect(getI18n()).toBeUndefined()
    expect(i18n.options.react?.useSuspense).toBe(false)
  })

  it('registers pt-BR as supported without shipping its catalog yet', () => {
    expect(SUPPORTED_LANGUAGES).toContain('pt-BR')
    expect(i18n.options.supportedLngs).toContain('pt-BR')
    expect(i18n.hasResourceBundle('pt-BR', 'nav')).toBe(false)
  })

  it('falls back to en for a language without translations', async () => {
    await i18n.changeLanguage('pt-BR')
    expect(i18n.language).toBe('pt-BR')
    expect(i18n.t('nav:projects')).toBe('Projects')
    expect(i18n.t('common:loading')).toBe('Loading…')
  })

  it('resolves keys across namespaces and nests terms', () => {
    // The code's `project` reads "Service"; the grouping entity reads
    // "Project" (ADR-048), and the main nav entry lists Projects.
    expect(i18n.t('nav:projects')).toBe('Projects')
    expect(i18n.t('nav:project.settingsTooltip')).toBe('Service settings')
    expect(i18n.t('nav:platform.settingsTooltip')).toBe('Platform settings')
    expect(i18n.t('nav:back.toProjects')).toBe('Back to projects')
    expect(i18n.t('nav:back.toProjectGroup', { name: 'CRM' })).toBe(
      'Back to CRM'
    )
    expect(i18n.t('nav:projectGroup.navigationLabel')).toBe(
      'Project navigation'
    )
    expect(i18n.t('projectGroups:switcher.switchLabel', { name: 'CRM' })).toBe(
      'CRM — Switch project'
    )
    expect(i18n.t('nav:projectGroup.moreServices', { count: 1 })).toBe(
      '1 more service not shown'
    )
    expect(i18n.t('nav:projectGroup.moreServices', { count: 4 })).toBe(
      '4 more services not shown'
    )
    expect(i18n.t('projectGroups:ungrouped')).toBe('Ungrouped services')
    expect(i18n.t('terms:project.singular')).toBe('Service')
    expect(i18n.t('terms:projectGroup.singular')).toBe('Project')
    expect(i18n.t('command:categories.externalService')).toBe('Database')
    expect(i18n.t('projects:list.total', { count: 1 })).toBe('1 service')
    expect(i18n.t('projects:list.total', { count: 3 })).toBe('3 services')
  })

  it('types keys against the en catalogs', () => {
    // A key that is not in locales/en/nav.json must not type-check; if this
    // directive ever becomes unused, `tsc --noEmit` fails the lint step.
    // @ts-expect-error -- missing key
    const missing = i18n.t('nav:doesNotExist')
    expect(missing).toBe('doesNotExist')
  })
})
