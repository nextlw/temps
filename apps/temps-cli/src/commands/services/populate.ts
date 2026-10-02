// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import type { Command } from 'commander'
import { requireAuth } from '../../config/store.js'
import { setupClient, getErrorMessage } from '../../lib/api-client.js'
import {
  startServicePopulate,
  listServicePopulateRuns,
  getServicePopulateRun,
} from '../../api/sdk.gen.js'
import type { PopulateRunListResponse, PopulateRunResponse } from '../../api/types.gen.js'
import {
  startSpinner,
  succeedSpinner,
  failSpinner,
  updateSpinner,
  withSpinner,
} from '../../ui/spinner.js'
import { printTable, statusBadge, type TableColumn } from '../../ui/table.js'
import { promptConfirm } from '../../ui/prompts.js'
import {
  newline,
  header,
  icons,
  json as jsonOut,
  colors,
  success,
  info,
  warning,
  error as errorOutput,
  keyValue,
  formatRelativeTime,
} from '../../ui/output.js'
import { formatBytes } from './restore.js'

/**
 * Environment variable the source URL can come from. The URL carries a
 * password, so it is never accepted as a command-line argument: arguments
 * end up in shell history and in `ps` output.
 */
export const SOURCE_URL_ENV = 'TEMPS_POPULATE_SOURCE_URL'

interface PopulateOptions {
  id: string
  database: string
  replace?: boolean
  sourceUrlStdin?: boolean
  yes?: boolean
  wait?: boolean
  json?: boolean
}

interface PopulateRunsOptions {
  id: string
  page?: string
  pageSize?: string
  json?: boolean
}

interface PopulateRunOptions {
  id: string
  run: string
  json?: boolean
}

/**
 * Pick the source URL from stdin (when `--source-url-stdin` was given) or the
 * environment. Returns an error message instead of throwing so the caller can
 * print it and exit.
 */
export function resolveSourceUrl(
  fromStdin: string | undefined,
  env: Record<string, string | undefined>,
): { url: string } | { error: string } {
  const candidate = fromStdin !== undefined ? fromStdin.trim() : env[SOURCE_URL_ENV]?.trim()
  if (!candidate) {
    return {
      error:
        fromStdin !== undefined
          ? 'No source URL on stdin.'
          : `No source URL. Pipe it with --source-url-stdin or set ${SOURCE_URL_ENV}.`,
    }
  }
  if (!/^postgres(ql)?:\/\//.test(candidate)) {
    return { error: 'The source URL must start with postgres:// or postgresql://.' }
  }
  return { url: candidate }
}

/** Replace user and password of a connection URL with `***` for display. */
export function maskConnectionUrl(raw: string): string {
  try {
    const url = new URL(raw)
    if (url.username) url.username = '***'
    if (url.password) url.password = '***'
    return url.toString()
  } catch {
    return '***'
  }
}

async function readStdin(): Promise<string> {
  let data = ''
  for await (const chunk of process.stdin) {
    data += typeof chunk === 'string' ? chunk : Buffer.from(chunk).toString('utf8')
  }
  return data
}

function parseId(raw: string, label: string): number {
  const value = Number.parseInt(raw, 10)
  if (!Number.isFinite(value) || value <= 0 || String(value) !== raw.trim()) {
    errorOutput(`Invalid ${label}: ${raw}`)
    process.exit(1)
  }
  return value
}

async function populateAction(options: PopulateOptions): Promise<void> {
  await requireAuth()
  await setupClient()

  const serviceId = parseId(options.id, 'service id')

  if (options.sourceUrlStdin && process.stdin.isTTY) {
    errorOutput(
      `--source-url-stdin reads a pipe, e.g. \`printf '%s' "$URL" | temps services populate ...\`; ` +
        `or set ${SOURCE_URL_ENV}.`,
    )
    process.exit(1)
  }
  const stdin = options.sourceUrlStdin ? await readStdin() : undefined
  const resolved = resolveSourceUrl(stdin, process.env)
  if ('error' in resolved) {
    errorOutput(resolved.error)
    process.exit(1)
  }

  if (options.replace && !options.yes) {
    if (!process.stdout.isTTY) {
      errorOutput('--replace drops the database first; pass --yes to confirm non-interactively.')
      process.exit(1)
    }
    newline()
    header(`${icons.arrow} Replace database '${options.database}' of service ${serviceId}`)
    keyValue('Source', maskConnectionUrl(resolved.url))
    newline()
    const go = await promptConfirm({
      message: `This DROPS '${options.database}' (disconnecting its clients) and copies the source into it. Continue?`,
      default: false,
    })
    if (!go) {
      warning('Aborted.')
      return
    }
  }

  const run = await withSpinner('Starting populate...', async () => {
    const { data, error } = await startServicePopulate({
      path: { id: serviceId },
      body: {
        database: options.database,
        source_url: resolved.url,
        replace: options.replace ?? false,
      },
    })
    if (error) throw new Error(getErrorMessage(error))
    return data as PopulateRunResponse
  })

  if (options.wait === false) {
    if (options.json) {
      jsonOut(run)
      return
    }
    success(`Populate run ${run.id} started.`)
    info(`Follow it with: temps services populate-run --id ${serviceId} --run ${run.id}`)
    return
  }

  if (!options.json) {
    success(`Populate run ${run.id} started (client ${run.client_image}).`)
  }
  const finalRun = await pollPopulateRun(serviceId, run.id, Boolean(options.json))

  if (options.json) {
    jsonOut(finalRun)
  } else {
    printRun(finalRun)
  }
  if (finalRun.status !== 'completed') {
    process.exitCode = 1
  }
}

async function pollPopulateRun(
  serviceId: number,
  runId: number,
  quiet: boolean,
): Promise<PopulateRunResponse> {
  if (!quiet) startSpinner(`Copying into the database (run ${runId})...`)
  // The server bounds a copy at 30 minutes; poll a little longer than that.
  const deadline = Date.now() + 35 * 60 * 1000
  let failures = 0
  const started = Date.now()
  while (Date.now() < deadline) {
    const { data, error } = await getServicePopulateRun({ path: { id: serviceId, run_id: runId } })
    if (error) {
      failures++
      if (failures > 5) {
        if (!quiet) failSpinner(`Failed to fetch run status: ${getErrorMessage(error)}`)
        throw new Error(getErrorMessage(error))
      }
    } else if (data) {
      failures = 0
      const run = data as PopulateRunResponse
      if (run.status !== 'running') {
        if (!quiet) {
          if (run.status === 'completed') succeedSpinner('Copy finished.')
          else failSpinner('Copy failed.')
        }
        return run
      }
      if (!quiet) {
        const elapsed = Math.round((Date.now() - started) / 1000)
        updateSpinner(`Copying into the database (run ${runId}, ${elapsed}s)...`)
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 2000))
  }
  if (!quiet) failSpinner('Timed out waiting for the copy to finish.')
  throw new Error(`Populate run ${runId} still running after 35 minutes`)
}

function printRun(run: PopulateRunResponse): void {
  newline()
  header(`${icons.info} Populate run ${run.id}`)
  keyValue('Status', run.status)
  keyValue('Database', run.database)
  keyValue('Source', run.source_url_masked)
  keyValue('Replace', run.replace ? 'yes' : 'no')
  keyValue('Client image', run.client_image)
  keyValue('Started', run.started_at)
  if (run.finished_at) keyValue('Finished', run.finished_at)
  if (run.duration_seconds != null) keyValue('Duration', `${run.duration_seconds.toFixed(1)}s`)
  if (run.database_size_bytes != null) keyValue('Size', formatBytes(run.database_size_bytes))
  if (run.error_message) keyValue('Error', colors.error(run.error_message))
  newline()
}

async function listRunsAction(options: PopulateRunsOptions): Promise<void> {
  await requireAuth()
  await setupClient()

  const serviceId = parseId(options.id, 'service id')
  const page = options.page ? parseId(options.page, 'page') : undefined
  const pageSize = options.pageSize ? parseId(options.pageSize, 'page size') : undefined

  const result = await withSpinner('Fetching populate runs...', async () => {
    const { data, error } = await listServicePopulateRuns({
      path: { id: serviceId },
      query: { page, page_size: pageSize },
    })
    if (error) throw new Error(getErrorMessage(error))
    return data as PopulateRunListResponse
  })

  if (options.json) {
    jsonOut(result)
    return
  }

  if (result.runs.length === 0) {
    info('No populate runs for this service yet.')
    return
  }

  const columns: TableColumn<PopulateRunResponse>[] = [
    { header: 'ID', accessor: (r) => String(r.id) },
    { header: 'Database', accessor: (r) => r.database },
    {
      header: 'Status',
      accessor: (r) => r.status,
      color: (_value, r) =>
        statusBadge(r.status === 'completed' ? 'active' : r.status === 'failed' ? 'inactive' : 'pending'),
    },
    { header: 'Replace', accessor: (r) => (r.replace ? 'yes' : 'no') },
    {
      header: 'Duration',
      accessor: (r) => (r.duration_seconds != null ? `${r.duration_seconds.toFixed(1)}s` : '—'),
    },
    {
      header: 'Size',
      accessor: (r) => (r.database_size_bytes != null ? formatBytes(r.database_size_bytes) : '—'),
    },
    { header: 'Started', accessor: (r) => formatRelativeTime(r.started_at) },
    {
      header: 'Error',
      accessor: (r) => (r.error_message ? r.error_message.slice(0, 60) : ''),
      color: (value) => colors.error(value),
    },
  ]
  newline()
  header(`${icons.info} Populate runs for service ${serviceId} (${result.total} total)`)
  printTable(result.runs, columns)
  newline()
}

async function showRunAction(options: PopulateRunOptions): Promise<void> {
  await requireAuth()
  await setupClient()

  const serviceId = parseId(options.id, 'service id')
  const runId = parseId(options.run, 'run id')

  const run = await withSpinner('Fetching populate run...', async () => {
    const { data, error } = await getServicePopulateRun({ path: { id: serviceId, run_id: runId } })
    if (error) throw new Error(getErrorMessage(error))
    return data as PopulateRunResponse
  })

  if (options.json) {
    jsonOut(run)
    return
  }
  printRun(run)
}

export function registerPopulateCommands(services: Command): void {
  services
    .command('populate')
    .description(
      'Copy an external PostgreSQL database into a database of a managed PostgreSQL service ' +
        `(admin only). The source URL is read from stdin (--source-url-stdin) or ${SOURCE_URL_ENV}, ` +
        'never from an argument.',
    )
    .requiredOption('--id <id>', 'Service ID')
    .requiredOption('--database <name>', 'Destination database, e.g. my_app_production')
    .option('--replace', 'Drop and recreate the destination when it already has tables')
    .option('--source-url-stdin', 'Read the source connection URL from stdin')
    .option('-y, --yes', 'Skip the --replace confirmation')
    .option('--no-wait', 'Return right after starting instead of following the run')
    .option('--json', 'Output in JSON format')
    .action(populateAction)

  services
    .command('populate-runs')
    .description('List the populate runs of a service, newest first')
    .requiredOption('--id <id>', 'Service ID')
    .option('--page <n>', 'Page (default 1)')
    .option('--page-size <n>', 'Page size (default 20, max 100)')
    .option('--json', 'Output in JSON format')
    .action(listRunsAction)

  services
    .command('populate-run')
    .description('Show one populate run')
    .requiredOption('--id <id>', 'Service ID')
    .requiredOption('--run <id>', 'Populate run ID')
    .option('--json', 'Output in JSON format')
    .action(showRunAction)
}
