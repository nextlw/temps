// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * Login do Portal da Infra.
 *
 * O visual é o do login do CRM (`dukk-front-web`), portado para React no
 * design system do Dukk — ver `components/auth/dukk/`. A lógica de sessão é a
 * do Temps e não mudou: MFA, troca de senha obrigatória e `return_to`
 * continuam sendo tratados exatamente como antes.
 */

import {
  emailStatusOptions,
  loginMutation,
} from '@/api/client/@tanstack/react-query.gen'
import { DukkAuthShell } from '@/components/auth/dukk/DukkAuthShell'
import { InfraLoginForm } from '@/components/auth/dukk/InfraLoginForm'
import { useAuth } from '@/contexts/AuthContext'
import { usePageTitle } from '@/hooks/usePageTitle'
import { consumeReturnTo } from '@/lib/return-to'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useMemo, useState } from 'react'
import { toast } from 'sonner'
import { useNavigate, useSearchParams } from 'react-router'

/**
 * Traduz os códigos opacos de erro de SSO (de `login_error_code_for` em
 * `oidc_handler.rs`) para mensagens legíveis. O servidor devolve códigos em vez
 * do texto cru do IdP justamente para não vazar a descrição de erro dele para a
 * URL / histórico / Referer do navegador. Código desconhecido cai na mensagem
 * genérica.
 *
 * Em português porque esta tela é o login do Portal da Infra — deixar o corpo
 * da página em português e o erro em inglês só apareceria no pior momento, que
 * é quando alguém não consegue entrar.
 */
const OIDC_ERROR_MESSAGES: Record<string, string> = {
  idp_error:
    'Seu provedor de identidade recusou o login. Verifique se sua conta tem acesso liberado.',
  idp_unreachable:
    'Não conseguimos falar com seu provedor de identidade. Tente de novo em instantes.',
  idp_rejected_code:
    'Seu provedor de identidade recusou o código de autorização. Inicie o login novamente.',
  state_invalid:
    'Este link de SSO é inválido ou já foi usado. Inicie o login novamente.',
  state_expired: 'Este link de SSO expirou. Inicie o login novamente.',
  id_token_invalid:
    'Seu provedor de identidade devolveu um token inválido. Contate o administrador.',
  callback_invalid:
    'O retorno do SSO veio malformado. Inicie o login novamente.',
  email_missing:
    'Seu provedor de identidade não devolveu um e-mail. Libere o escopo "email" e tente de novo.',
  email_not_verified:
    'Seu provedor de identidade ainda não confirmou seu e-mail. Verifique no provedor e tente de novo.',
  user_not_provisioned:
    'Não existe conta neste portal para este e-mail. Peça a um administrador para criá-la.',
  provider_disabled: 'Este provedor de SSO está desativado no momento.',
  provider_not_found: 'A configuração do provedor de SSO não foi encontrada.',
  no_provider_configured:
    'Nenhum provedor de SSO está configurado neste portal.',
  issuer_invalid: 'A URL do provedor de SSO é inválida.',
  return_to_invalid: 'Destino de redirecionamento pós-login inválido.',
  role_invalid: 'O papel atribuído pelo provedor de SSO é inválido.',
  role_mapping_not_found: 'Nenhum mapeamento de papel do SSO corresponde.',
  provider_conflict: 'Conflito na configuração do provedor de SSO.',
  provider_managed_by_cloud:
    'Este provedor de SSO é gerenciado pelo Temps Cloud e não pode ser editado aqui.',
  insufficient_role:
    'Sua conta não tem papel de owner ou admin nesta instância. Peça a um administrador para conceder e tente de novo.',
  issuer_managed_by_cloud:
    'Este issuer já é usado pelo provedor gerenciado pelo Temps Cloud. Entre por ele ou desfaça o vínculo antes de cadastrar um provedor próprio com o mesmo issuer.',
  internal_error: 'Ocorreu um erro interno ao processar o retorno do SSO.',
}

function oidcErrorMessage(reason: string | null): string {
  if (!reason) return 'Não foi possível entrar com o SSO.'
  return OIDC_ERROR_MESSAGES[reason] ?? 'Não foi possível entrar com o SSO.'
}

export const Login = () => {
  usePageTitle('Entrar')
  const [isLoading, setIsLoading] = useState(false)
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { refetch } = useAuth()
  const [searchParams] = useSearchParams()

  const { data: emailStatus } = useQuery(emailStatusOptions())

  const oidcError = useMemo(() => {
    if (searchParams.get('error') !== 'oidc_failed') {
      return null
    }
    const reason = searchParams.get('reason')
    return oidcErrorMessage(reason)
  }, [searchParams])

  const login = useMutation({
    ...loginMutation(),
    meta: {
      errorTitle: 'Falha no login',
    },
    onSuccess: async (data) => {
      if (data.password_change_required) {
        navigate('/auth/change-password', { replace: true })
        return
      }

      if (data.mfa_required) {
        toast.success('Conclua a verificação em duas etapas')
        navigate('/mfa-verify')
        return
      }

      if (data.mfa_enrollment_required && data.mfa_setup) {
        toast.success('Configure a verificação em duas etapas para continuar')
        navigate('/auth/change-password', {
          replace: true,
          state: { mfaSetup: data.mfa_setup },
        })
        return
      }

      toast.success('Login realizado')
      await queryClient.invalidateQueries({ queryKey: ['getCurrentUser'] })
      await refetch()
      navigate(consumeReturnTo('/dashboard'), { replace: true })
    },
  })

  const handleSubmit = async (data: { email: string; password: string }) => {
    setIsLoading(true)
    try {
      await login.mutateAsync({
        body: data,
      })
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <DukkAuthShell>
      <InfraLoginForm
        onSubmit={handleSubmit}
        isLoading={isLoading || login.isPending}
        oidcProviders={emailStatus?.oidc_providers ?? []}
        externalError={oidcError}
        passwordResetAvailable={emailStatus?.password_reset_available ?? false}
        /*
         * Defaults to `true` while the query is in flight and if an older
         * server omits the field. Erring the other way would flash an
         * SSO-only screen at an instance that accepts passwords, and on a
         * server without SSO configured that screen has no way in at all.
         */
        passwordLoginEnabled={emailStatus?.password_login_enabled ?? true}
      />
    </DukkAuthShell>
  )
}
