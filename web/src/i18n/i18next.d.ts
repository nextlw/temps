// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Types every `t()` key against the `en` catalogs: a key missing from
// `locales/en/*.json` is a type error, not a string rendered at runtime.

import 'i18next'
import type common from './locales/en/common.json'
import type nav from './locales/en/nav.json'
import type terms from './locales/en/terms.json'
import type command from './locales/en/command.json'
import type projects from './locales/en/projects.json'
import type observability from './locales/en/observability.json'
import type audit from './locales/en/audit.json'
import type storage from './locales/en/storage.json'
import type ai from './locales/en/ai.json'
import type projectGroups from './locales/en/projectGroups.json'

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'common'
    resources: {
      common: typeof common
      nav: typeof nav
      terms: typeof terms
      command: typeof command
      projects: typeof projects
      observability: typeof observability
      audit: typeof audit
      storage: typeof storage
      ai: typeof ai
      projectGroups: typeof projectGroups
    }
  }
}
