import { SettingsSection } from '@/components/ui/settings-section'
import { FolderPen, GitFork, FileCode, Trash2 } from 'lucide-react'
// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { ProjectResponse } from '@/api/client'
import {
  deleteProjectMutation,
  updateProjectSettingsMutation,
} from '@/api/client/@tanstack/react-query.gen'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { ConfirmNameBadge } from '@/components/ui/confirm-name-badge'
import { CloudTelemetryBackfillCard } from './CloudTelemetryBackfillCard'
import { ProjectGroupMembershipSection } from '@/components/project-groups/ProjectGroupMembershipSection'
import { MonitoringCard } from './MonitoringCard'
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { useSensitiveActionVerification } from '@/hooks/useSensitiveActionVerification'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { zodResolver } from '@hookform/resolvers/zod'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'
import { useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { i18n } from '@/i18n'
import { toast } from 'sonner'
import { z } from 'zod'

interface GeneralSettingsProps {
  project: ProjectResponse
  refetch: () => void
}

const projectSchema = z.object({
  name: z
    .string()
    .trim()
    .min(1, i18n.t('projects:settings.general.nameRequired'))
    .max(100, i18n.t('projects:settings.general.nameTooLong')),
  slug: z
    .string()
    .trim()
    .min(1, i18n.t('projects:settings.general.slugRequired'))
    .max(63, i18n.t('projects:settings.general.slugTooLong'))
    .regex(
      /^[a-z0-9]+(?:-[a-z0-9]+)*$/,
      'Use lowercase letters, numbers and single hyphens (no leading or trailing hyphen)'
    ),
})

type ProjectFormValues = z.infer<typeof projectSchema>

export function GeneralSettings({ project, refetch }: GeneralSettingsProps) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { t } = useTranslation('projects', { keyPrefix: 'settings.general' })
  // Renaming a project onto — or off — a slug this host grants the Docker
  // socket to is a sensitive action (ADR 045), so the save can come back 428
  // asking the admin to re-verify rather than failing. Every other save here
  // is unaffected and never reaches the dialog.
  const { handleSensitiveActionError, verificationDialog } =
    useSensitiveActionVerification()

  const updateProjectSettings = useMutation({
    ...updateProjectSettingsMutation(),
    meta: {
      errorTitle: t('updateSettingsFailed'),
    },
  })

  const projectForm = useForm<ProjectFormValues>({
    resolver: zodResolver(projectSchema),
    defaultValues: {
      name: project?.name || '',
      slug: project?.slug || '',
    },
  })

  // `defaultValues` are only read on mount, but this component stays mounted
  // when the route switches between two projects' settings pages. Without this
  // reset the form would still hold the previous project's identity, and Save
  // would rename the newly-selected project to the old one's name and slug.
  //
  // Keyed on the project *identity*, not its values: a plain refetch of the
  // same project must not overwrite whatever the user is currently typing.
  const syncedProjectId = useRef<number | undefined>(undefined)
  useEffect(() => {
    if (project?.id === undefined || syncedProjectId.current === project.id) {
      return
    }
    syncedProjectId.current = project.id
    projectForm.reset({
      name: project.name || '',
      slug: project.slug || '',
    })
  }, [project?.id, project?.name, project?.slug, projectForm])

  const handleSaveProject = async (values: ProjectFormValues) => {
    if (!project?.id) return

    const request = updateProjectSettings.mutateAsync({
      path: { project_id: project.id! },
      body: {
        name: values.name,
        slug: values.slug,
      },
    })
    // Hand-rolled rather than `toast.promise`, because one outcome is neither
    // success nor failure: a 428 means "prove it's you and this will go
    // through". Attaching a fixed error toast up front would flash a red
    // "Failed to update project" behind the verification dialog for a save
    // that is about to succeed.
    const toastId = toast.loading(t('updating'))
    let updated
    try {
      updated = await request
    } catch (error) {
      toast.dismiss(toastId)
      // Opens the step-up dialog and re-runs this save once verified. The
      // global mutation handler already suppresses its own toast for
      // STEP_UP_REQUIRED, so nothing else fires in the meantime.
      if (
        handleSensitiveActionError(error, () => void handleSaveProject(values))
      ) {
        return
      }
      const problem = error as { detail?: string; message?: string }
      toast.error(problem.detail || problem.message || t('updateFailed'))
      return
    }
    toast.success(t('updated'), { id: toastId })
    refetch()
    // Navigate to the slug the server persisted, not the one submitted: the
    // server normalizes it, so routing on the raw input can land on a URL that
    // does not exist.
    navigate(`/projects/${updated?.slug ?? values.slug}/settings/general`)
  }

  const handleToggleCrossProjectTraceSharing = async (enabled: boolean) => {
    if (!project?.id) return

    await toast.promise(
      updateProjectSettings.mutateAsync({
        path: { project_id: project.id! },
        body: {
          cross_project_trace_sharing: enabled,
        },
      }),
      {
        loading: t('traceSharingUpdating'),
        success: t('traceSharingUpdated'),
        error: t('traceSharingFailed'),
      }
    )
    refetch()
  }

  const handleToggleErrorSourceContext = async (enabled: boolean) => {
    if (!project?.id) return

    await toast.promise(
      updateProjectSettings.mutateAsync({
        path: { project_id: project.id! },
        body: {
          error_source_context_enabled: enabled,
        },
      }),
      {
        loading: 'Updating source context setting...',
        success: 'Error tracking source context updated',
        error: 'Failed to update source context setting',
      }
    )
    refetch()
  }

  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false)
  const [deleteConfirmName, setDeleteConfirmName] = useState('')
  const deleteProjectMutationM = useMutation({
    ...deleteProjectMutation(),
    meta: {
      errorTitle: t('deleteFailed'),
    },
  })

  const handleDeleteProject = async () => {
    if (deleteConfirmName.trim() !== project?.name) return
    setIsDeleteDialogOpen(false)
    try {
      await toast.promise(
        deleteProjectMutationM.mutateAsync({
          path: { id: project.id! },
        }),
        {
          loading: t('deleting'),
          success: () => {
            // Lists of services, the Projects' catalogue among them.
            void queryClient.invalidateQueries({ queryKey: ['getProjects'] })
            navigate('/projects')
            return t('deleted')
          },
          error: t('deleteFailed'),
        }
      )
    } catch (error) {
      console.error('Error deleting project:', error)
    }
  }

  return (
    <div className="space-y-3">
      {verificationDialog}
      {/* Project Settings Card */}
      <Form {...projectForm}>
        <form onSubmit={projectForm.handleSubmit(handleSaveProject)}>
          <SettingsSection
            title={t('identityTitle')}
            icon={FolderPen}
            defaultOpen
          >
            <div className="space-y-6">
              <FormField
                control={projectForm.control}
                name="name"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('nameLabel')}</FormLabel>
                    <FormControl>
                      <Input {...field} className="max-w-[400px]" />
                    </FormControl>
                    <FormDescription className="text-muted-foreground">
                      {t('nameDescription')}
                    </FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />

              <FormField
                control={projectForm.control}
                name="slug"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('slugLabel')}</FormLabel>
                    <FormControl>
                      <Input {...field} className="max-w-[400px]" />
                    </FormControl>
                    <FormDescription className="text-muted-foreground">
                      {t('slugDescription')}
                    </FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>
            <div className="pt-4">
              <Button
                size="sm"
                type="submit"
                disabled={updateProjectSettings.isPending}
              >
                Save
              </Button>
            </div>
          </SettingsSection>
        </form>
      </Form>

      {/* The Project this service ships with (ADR-049) */}
      <ProjectGroupMembershipSection project={project} />

      {/* Monitoring — what deployments report about themselves */}
      <MonitoringCard project={project} refetch={refetch} />

      {/* ADR-040 — where this project's telemetry history stands with Temps
          Cloud. Always rendered: the backfill is a deliberate CLI action, so
          this card is the only place it is discoverable from the Console. */}
      <CloudTelemetryBackfillCard project={project} />

      {/* Cross-Project Trace Sharing Card */}
      <SettingsSection title="Trace sharing" icon={GitFork}>
        <div>
          <div className="flex flex-row items-center justify-between rounded-lg border p-4">
            <div className="space-y-0.5 pr-4">
              <Label className="text-base">{t('traceSharingLabel')}</Label>
              <p className="text-sm text-muted-foreground">
                {t('traceSharingDescription')}
              </p>
            </div>
            <Switch
              checked={project?.cross_project_trace_sharing ?? true}
              onCheckedChange={handleToggleCrossProjectTraceSharing}
              disabled={updateProjectSettings.isPending}
            />
          </div>
        </div>
      </SettingsSection>

      {/* Error Tracking Source Context Card */}
      <SettingsSection title="Source context" icon={FileCode}>
        <div>
          <div className="flex flex-row items-center justify-between rounded-lg border p-4">
            <div className="space-y-0.5 pr-4">
              <Label className="text-base">Source code in stack traces</Label>
              <p className="text-sm text-muted-foreground">
                Off by default. When enabled, upload your application source per
                release (via the CLI or API, keyed by the deployed commit/tag)
                and Temps shows the code around each frame. Source files are
                only accepted and stored while this is on.
              </p>
            </div>
            <Switch
              checked={project?.error_source_context_enabled ?? false}
              onCheckedChange={handleToggleErrorSourceContext}
              disabled={updateProjectSettings.isPending}
            />
          </div>
        </div>
      </SettingsSection>

      {/* Danger Zone */}
      <SettingsSection title={t('deleteTitle')} icon={Trash2}>
        <p className="text-sm text-muted-foreground mt-1 mb-4">
          {t('deleteDescription')}
        </p>
        <AlertDialog
          open={isDeleteDialogOpen}
          onOpenChange={(open) => {
            setIsDeleteDialogOpen(open)
            if (!open) setDeleteConfirmName('')
          }}
        >
          <AlertDialogTrigger asChild>
            <Button variant="destructive">{t('deleteButton')}</Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Are you absolutely sure?</AlertDialogTitle>
              <AlertDialogDescription>
                {t('deleteConfirmDescription')}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <div className="space-y-2">
              <Label htmlFor="confirm-delete-project-name">
                Type <ConfirmNameBadge value={project?.name ?? ''} /> to confirm
              </Label>
              <Input
                id="confirm-delete-project-name"
                value={deleteConfirmName}
                onChange={(e) => setDeleteConfirmName(e.target.value)}
                placeholder={project?.name}
                autoComplete="off"
              />
            </div>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction
                onClick={handleDeleteProject}
                disabled={
                  deleteProjectMutationM.isPending ||
                  deleteConfirmName.trim() !== project?.name
                }
                className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              >
                {deleteProjectMutationM.isPending ? 'Deleting...' : 'Delete'}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </SettingsSection>
    </div>
  )
}
