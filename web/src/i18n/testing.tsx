// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Test helper: `react-dom/server`'s renderToStaticMarkup inside the console's
// <I18nextProvider>. Console components read their copy through
// useTranslation, which renders raw keys without the provider (the console
// registers no global instance, see ./index.ts).

import type { ReactNode } from 'react'
import { renderToStaticMarkup as renderMarkup } from 'react-dom/server'
import { I18nextProvider } from 'react-i18next'
import { i18n } from './index'

export function renderToStaticMarkup(node: ReactNode): string {
  return renderMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>)
}
