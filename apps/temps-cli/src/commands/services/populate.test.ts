// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { test, expect, describe } from 'bun:test'
import {
  maskConnectionUrl,
  replaceConfirmation,
  resolveSourceUrl,
  SOURCE_URL_ENV,
} from './populate.js'

describe('resolveSourceUrl', () => {
  const url = 'postgres://app:s3cret@db.example.com:5432/app?sslmode=require'

  test('takes the URL from stdin, trimming the trailing newline', () => {
    expect(resolveSourceUrl(`${url}\n`, {})).toEqual({ url })
  })

  test('stdin wins over the environment', () => {
    expect(resolveSourceUrl(url, { [SOURCE_URL_ENV]: 'postgres://other/db' })).toEqual({ url })
  })

  test('falls back to the environment variable', () => {
    expect(resolveSourceUrl(undefined, { [SOURCE_URL_ENV]: url })).toEqual({ url })
  })

  test('explains both sources when neither has a URL', () => {
    const result = resolveSourceUrl(undefined, {})
    expect('error' in result && result.error).toContain('--source-url-stdin')
    expect('error' in result && result.error).toContain(SOURCE_URL_ENV)
  })

  test('an empty stdin is an error, not a silent fallback', () => {
    expect(resolveSourceUrl('  \n', { [SOURCE_URL_ENV]: url })).toEqual({
      error: 'No source URL on stdin.',
    })
  })

  test('refuses anything but a PostgreSQL URL', () => {
    const result = resolveSourceUrl('mysql://u:p@h/db', {})
    expect('error' in result).toBe(true)
  })
})

describe('maskConnectionUrl', () => {
  test('hides user and password, keeps host, database and sslmode', () => {
    const masked = maskConnectionUrl('postgres://app:s3cret@db.example.com:5432/app?sslmode=require')
    expect(masked).not.toContain('s3cret')
    expect(masked).not.toContain('app:')
    expect(masked).toContain('db.example.com:5432/app')
    expect(masked).toContain('sslmode=require')
  })

  test('never echoes an unparseable value', () => {
    expect(maskConnectionUrl('not a url s3cret')).toBe('***')
  })
})

describe('replaceConfirmation', () => {
  const base = {
    replace: true,
    yes: false,
    confirmDatabase: undefined as string | undefined,
    database: 'app_production',
    interactive: true,
  }

  test('without --replace nothing is asked', () => {
    expect(replaceConfirmation({ ...base, replace: false })).toEqual({ action: 'none' })
  })

  test('interactive --replace asks to type the name', () => {
    expect(replaceConfirmation(base)).toEqual({ action: 'prompt' })
  })

  test('--yes alone is not enough', () => {
    const result = replaceConfirmation({ ...base, yes: true })
    expect('error' in result && result.error).toContain('--confirm-database app_production')
  })

  test('--yes with a matching --confirm-database goes through', () => {
    expect(
      replaceConfirmation({ ...base, yes: true, confirmDatabase: 'app_production' }),
    ).toEqual({ action: 'none' })
  })

  test('a mismatching --confirm-database is refused', () => {
    const result = replaceConfirmation({ ...base, yes: true, confirmDatabase: 'app_homolog' })
    expect('error' in result && result.error).toContain('does not match')
  })

  test('without a terminal and without --yes it refuses instead of prompting', () => {
    const result = replaceConfirmation({ ...base, interactive: false })
    expect('error' in result).toBe(true)
  })

  test('--confirm-database without --replace is a mistake', () => {
    const result = replaceConfirmation({ ...base, replace: false, confirmDatabase: 'x' })
    expect('error' in result).toBe(true)
  })
})
