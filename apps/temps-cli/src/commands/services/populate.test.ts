// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { test, expect, describe } from 'bun:test'
import { maskConnectionUrl, resolveSourceUrl, SOURCE_URL_ENV } from './populate.js'

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
