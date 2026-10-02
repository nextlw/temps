// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { describe, expect, test } from 'bun:test'

import { extractServiceTypeFromImage } from './service-type-detector'

describe('extractServiceTypeFromImage', () => {
  test('recognizes official and source-built MinIO images', () => {
    for (const image of [
      'minio/minio:latest',
      'quay.io/minio/minio:RELEASE.2025-09-07T16-13-09Z',
      'ghcr.io/nextlw/minio:RELEASE.2025-09-07T16-13-09Z',
      'ghcr.io/nextlw/minio:RELEASE.2025-09-07T16-13-09Z@sha256:ab56307e607a5ad52647fd26942164c8816252fb279daead61078e174cad6e64',
      'ghcr.io/nextlw/minio@sha256:ab56307e607a5ad52647fd26942164c8816252fb279daead61078e174cad6e64',
    ]) {
      expect(extractServiceTypeFromImage(image)).toBe('minio')
    }
  })

  test('keeps the other engines', () => {
    expect(extractServiceTypeFromImage('postgres:18-alpine')).toBe('postgres')
    expect(extractServiceTypeFromImage('rustfs/rustfs:1.0.0')).toBe('rustfs')
    expect(extractServiceTypeFromImage('mysql:8')).toBe('mariadb')
    expect(extractServiceTypeFromImage('ghcr.io/nextlw/mc:latest')).toBeNull()
  })
})
