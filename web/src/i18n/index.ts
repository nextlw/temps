// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Console i18n. `en` is the source language and the only one with resources;
// `pt-BR` is registered so the language plumbing exists before its catalog
// does, and every missing key falls back to `en`. The console owns its own
// instance and registers it nowhere globally: neither as the `i18next` default
// nor as react-i18next's default (no `initReactI18next`). <TempsConsole>
// hands it to components through <I18nextProvider>, so a host edition that
// embeds the console with its own i18next setup shares neither catalogs nor
// the active language with it. Rendering a console component outside that
// provider (e.g. in a test) needs the same <I18nextProvider i18n={i18n}>.

import i18next from 'i18next'
import common from './locales/en/common.json'
import nav from './locales/en/nav.json'
import terms from './locales/en/terms.json'

export const FALLBACK_LANGUAGE = 'en'
export const SUPPORTED_LANGUAGES = ['en', 'pt-BR'] as const
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number]

export const NAMESPACES = ['common', 'nav', 'terms'] as const
export const DEFAULT_NAMESPACE = 'common'

export const resources = {
  en: { common, nav, terms },
} as const

export const i18n = i18next.createInstance()

void i18n.init({
  resources,
  lng: FALLBACK_LANGUAGE,
  fallbackLng: FALLBACK_LANGUAGE,
  supportedLngs: [...SUPPORTED_LANGUAGES],
  ns: [...NAMESPACES],
  defaultNS: DEFAULT_NAMESPACE,
  // Resources are bundled, so initialisation can finish synchronously and
  // the first render already has every string.
  initAsync: false,
  interpolation: {
    // React escapes rendered values already.
    escapeValue: false,
  },
  // Read by useTranslation through `i18n.options.react`.
  react: {
    useSuspense: false,
  },
})

export default i18n
