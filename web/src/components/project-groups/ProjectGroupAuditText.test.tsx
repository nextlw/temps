// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { I18nextProvider } from 'react-i18next'
import { AuditLogItemRow } from '@/components/audit/AuditLogItem'
import { i18n } from '@/i18n'

test('an audit row of a Project says Project, translated at render', () => {
  const markup = renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <table>
        <tbody>
          <AuditLogItemRow
            id={1}
            audit_date={0}
            operation_type="PROJECT_GROUP_DELETED"
            data={{ group_id: 3, name: 'CRM', slug: 'crm' }}
          />
        </tbody>
      </table>
    </I18nextProvider>
  )
  expect(markup).toContain('>Project<')
  expect(markup).toContain('Deleted project CRM; its services became ungrouped')
  expect(markup).not.toContain('>Service<')
})
