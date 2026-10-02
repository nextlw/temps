// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// Pure pieces of a Project's pages (UI: Project, code: `project_group`,
// docs/adr/049-project-groups.md): which services it lists, which services
// can be added to it and from where, and the state words of its overview.

import type { StatusTone } from '@temps-sdk/ds'
import { groupOfService, groupServices } from './project-groups'
import type { ProjectGroupResponse } from './project-groups-types'

interface NamedService {
  id: number
  name: string
}

const byServiceName = (a: NamedService, b: NamedService) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
  a.id - b.id

/** The group's services found in `catalog`, by name. */
export function servicesOfGroup<S extends NamedService>(
  group: ProjectGroupResponse,
  catalog: readonly S[]
): S[] {
  return groupServices([group], catalog).groups[0].services.sort(byServiceName)
}

export interface AddableService<S> {
  service: S
  /** The Project the service would leave; absent for an ungrouped service. */
  currentGroup?: ProjectGroupResponse
}

/**
 * The services that can join `group`: every service in `catalog` not already
 * in it, ungrouped ones first (adding them moves nothing), then the rest by
 * name, each with the Project it would be moved out of.
 */
export function addableServices<S extends NamedService>(
  groups: readonly ProjectGroupResponse[],
  catalog: readonly S[],
  group: ProjectGroupResponse
): AddableService<S>[] {
  const members = new Set(group.service_ids)
  return catalog
    .filter((service) => !members.has(service.id))
    .map((service) => ({
      service,
      currentGroup: groupOfService(groups, service.id),
    }))
    .sort(
      (a, b) =>
        Number(a.currentGroup !== undefined) -
          Number(b.currentGroup !== undefined) ||
        byServiceName(a.service, b.service)
    )
}

export type EnvironmentState = 'live' | 'sleeping' | 'notDeployed'

export function environmentState(environment: {
  sleeping: boolean
  current_deployment_id?: number | null
}): EnvironmentState {
  if (environment.sleeping) return 'sleeping'
  return environment.current_deployment_id ? 'live' : 'notDeployed'
}

export const ENVIRONMENT_STATE_TONE: Record<EnvironmentState, StatusTone> = {
  live: 'ok',
  sleeping: 'idle',
  notDeployed: 'idle',
}

export type DeploymentOutcome =
  'completed' | 'failed' | 'cancelled' | 'inProgress' | 'other'

const IN_PROGRESS = new Set([
  'pending',
  'queued',
  'running',
  'building',
  'deploying',
])

export function deploymentOutcome(status: string): DeploymentOutcome {
  const normalized = status.toLowerCase()
  if (normalized === 'completed' || normalized === 'success') {
    return 'completed'
  }
  if (normalized === 'failed' || normalized === 'error') return 'failed'
  if (normalized === 'cancelled' || normalized === 'canceled') {
    return 'cancelled'
  }
  if (IN_PROGRESS.has(normalized)) return 'inProgress'
  return 'other'
}

export const DEPLOYMENT_OUTCOME_TONE: Record<DeploymentOutcome, StatusTone> = {
  completed: 'ok',
  failed: 'error',
  cancelled: 'idle',
  inProgress: 'running',
  other: 'warn',
}
