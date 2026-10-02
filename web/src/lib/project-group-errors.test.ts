// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { describe, expect, it } from 'bun:test'
import {
  ProjectGroupRequestError,
  projectGroupErrorReason,
  projectGroupNameProblem,
} from './project-group-errors'

describe('projectGroupErrorReason', () => {
  it('reads the HTTP status kept beside the problem body', () => {
    const reason = (status: number) =>
      projectGroupErrorReason(new ProjectGroupRequestError(status, {}))
    expect(reason(400)).toBe('invalid')
    expect(reason(403)).toBe('forbidden')
    expect(reason(404)).toBe('notFound')
    expect(reason(409)).toBe('conflict')
    expect(reason(500)).toBe('unknown')
    expect(reason(0)).toBe('unknown')
  })
  it('does not guess from anything else', () => {
    expect(projectGroupErrorReason({ status: 409 })).toBe('unknown')
    expect(projectGroupErrorReason(new Error('boom'))).toBe('unknown')
    expect(projectGroupErrorReason(undefined)).toBe('unknown')
  })
})

describe('projectGroupNameProblem', () => {
  it('asks for a name and caps its length after trimming', () => {
    expect(projectGroupNameProblem('  ')).toBe('required')
    expect(projectGroupNameProblem(' CRM ')).toBeUndefined()
    expect(projectGroupNameProblem('x'.repeat(255))).toBeUndefined()
    expect(projectGroupNameProblem('x'.repeat(256))).toBe('tooLong')
    // 255 emoji are 510 UTF-16 units but 255 characters.
    expect(projectGroupNameProblem('😀'.repeat(255))).toBeUndefined()
    expect(projectGroupNameProblem('😀'.repeat(256))).toBe('tooLong')
  })
})
