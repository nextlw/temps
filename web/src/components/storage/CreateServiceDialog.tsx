// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import { useTranslation } from 'react-i18next'
import { CreatableServiceTypeRoute, CreateServiceResponse } from '@/api/client'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { ServiceCreationSuccessMessage } from '@/lib/service-creation-success'
import { CreateServiceForm } from './CreateServiceForm'

interface CreateServiceDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  serviceType: CreatableServiceTypeRoute
  onSuccess: (data: CreateServiceResponse) => void
  successMessage?: ServiceCreationSuccessMessage<CreateServiceResponse>
}

export function CreateServiceDialog({
  open,
  onOpenChange,
  serviceType,
  onSuccess,
  successMessage,
}: CreateServiceDialogProps) {
  const { t } = useTranslation('storage')
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t('create.title', { type: serviceType })}</DialogTitle>
        </DialogHeader>
        <CreateServiceForm
          serviceType={serviceType}
          onCancel={() => onOpenChange(false)}
          onSuccess={onSuccess}
          successMessage={successMessage}
        />
      </DialogContent>
    </Dialog>
  )
}
