// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Types every `t()` key against the `en` catalogs: a key missing from
// `locales/en/*.json` is a type error, not a string rendered at runtime.

import 'i18next'
import type common from './locales/en/common.json'
import type nav from './locales/en/nav.json'
import type terms from './locales/en/terms.json'

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'common'
    resources: {
      common: typeof common
      nav: typeof nav
      terms: typeof terms
    }
  }
}
