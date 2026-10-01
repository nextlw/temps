// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Failures of the project-group mutations (docs/adr/049-project-groups.md).
// Problem bodies carry no `status` field (temps-core serializes only what a
// handler sets), so the HTTP status is kept beside the body here and the UI
// picks its own translated message from it instead of showing the server's
// English `detail`.

export class ProjectGroupRequestError extends Error {
  readonly status: number
  readonly problem: unknown

  constructor(status: number, problem: unknown) {
    super(`project group request failed with HTTP ${status}`)
    this.name = 'ProjectGroupRequestError'
    this.status = status
    this.problem = problem
  }
}

/**
 * Why a project-group request failed, as the ADR's status table reads:
 * 400 invalid name or body, 403 missing permission or a hidden service,
 * 404 group or service gone, 409 slug taken.
 */
export type ProjectGroupErrorReason =
  'invalid' | 'forbidden' | 'notFound' | 'conflict' | 'unknown'

export function projectGroupErrorReason(
  error: unknown
): ProjectGroupErrorReason {
  if (!(error instanceof ProjectGroupRequestError)) return 'unknown'
  switch (error.status) {
    case 400:
    case 422:
      return 'invalid'
    case 403:
      return 'forbidden'
    case 404:
      return 'notFound'
    case 409:
      return 'conflict'
    default:
      return 'unknown'
  }
}

/** Longest name the API accepts (ADR-049, "Details fixed during implementation"). */
export const PROJECT_GROUP_NAME_MAX = 255

/** Checks a name before it is sent, so the field can say what is wrong. */
export function projectGroupNameProblem(
  name: string
): 'required' | 'tooLong' | undefined {
  const trimmed = name.trim()
  if (trimmed.length === 0) return 'required'
  if (trimmed.length > PROJECT_GROUP_NAME_MAX) return 'tooLong'
  return undefined
}
