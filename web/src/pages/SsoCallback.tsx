// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * Tela de retorno do SSO — onde o navegador aterrissa depois que o servidor
 * trocou o code por tokens e criou a sessão.
 *
 * O servidor faz `Redirect::to(&return_to)` no fim de `/api/auth/oidc/callback`
 * e a tela de handoff pediu `return_to=/auth/sso/callback`, então esta rota é o
 * primeiro pixel que o operador vê ao voltar do Zitadel. Antes disso ele caía
 * direto no painel, que é o único trecho do trajeto que ainda não estava no DS.
 *
 * A sessão JÁ existe quando esta tela monta (o cookie veio no mesmo 302), então
 * aqui não há troca de code: o trabalho é revalidar o usuário em cache e seguir
 * para o destino original guardado no `sessionStorage`. Se a revalidação falhar,
 * a tela diz isso e oferece o caminho de volta ao login em vez de deixar o
 * operador num carregamento infinito.
 *
 * Rota pública de propósito: ela roda no instante em que a sessão acabou de
 * nascer, e exigir autenticação para exibi-la criaria uma corrida com o
 * próprio `refetch` que ela dispara.
 */

import { DukkAuthShell } from '@/components/auth/dukk/DukkAuthShell'
import { useAuth } from '@/contexts/AuthContext'
import { usePageTitle } from '@/hooks/usePageTitle'
import { consumeReturnTo } from '@/lib/return-to'
import { useQueryClient } from '@tanstack/react-query'
import { AlertCircle, Loader2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'

export const SsoCallback = () => {
  usePageTitle('Concluindo login')
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { refetch } = useAuth()
  const [error, setError] = useState<string | null>(null)
  /**
   * `refetch` e `queryClient` trocam de identidade entre renders, então sem
   * esta guarda o efeito rodaria de novo e dispararia uma segunda navegação —
   * inclusive depois do `consumeReturnTo`, que é destrutivo por natureza.
   */
  const started = useRef(false)

  useEffect(() => {
    if (started.current) return
    started.current = true

    const finish = async () => {
      try {
        await queryClient.invalidateQueries({ queryKey: ['getCurrentUser'] })
        await refetch()
        navigate(consumeReturnTo('/dashboard'), { replace: true })
      } catch (e) {
        setError(
          e instanceof Error
            ? e.message
            : 'Não foi possível confirmar sua sessão.'
        )
      }
    }

    void finish()
  }, [navigate, queryClient, refetch])

  return (
    <DukkAuthShell>
      <div
        className="dukk-fadeup"
        style={{ width: '100%', maxWidth: '400px' }}
        aria-live="polite"
      >
        {error ? (
          <>
            <div
              role="alert"
              style={{
                display: 'flex',
                alignItems: 'flex-start',
                gap: '8px',
                background: 'rgb(var(--c-danger) / 0.1)',
                border: '1px solid rgb(var(--c-danger) / 0.3)',
                borderRadius: '10px',
                padding: '11px 13px',
                marginBottom: '18px',
              }}
            >
              <AlertCircle
                width={15}
                height={15}
                color="rgb(var(--c-danger))"
                style={{ flexShrink: 0, marginTop: '2px' }}
                aria-hidden="true"
              />
              <span
                style={{
                  fontSize: '13px',
                  color: 'rgb(var(--c-danger))',
                  lineHeight: 1.4,
                }}
              >
                {error}
              </span>
            </div>
            <button
              type="button"
              style={{
                width: '100%',
                boxSizing: 'border-box',
                border: '1px solid rgb(var(--c-line))',
                background: 'rgb(var(--c-paper))',
                color: 'rgb(var(--c-ink))',
                padding: '11px 16px',
                borderRadius: '8px',
                fontFamily: 'var(--font-body)',
                fontSize: '14px',
                fontWeight: 600,
                cursor: 'pointer',
              }}
              onClick={() => navigate('/login', { replace: true })}
            >
              Voltar ao login
            </button>
          </>
        ) : (
          <>
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: '10px',
                marginBottom: '16px',
              }}
            >
              <Loader2
                className="dukk-spin"
                width={18}
                height={18}
                color="rgb(var(--c-lime))"
                aria-hidden="true"
              />
              <span
                style={{
                  fontSize: '11.5px',
                  fontWeight: 600,
                  letterSpacing: '0.04em',
                  textTransform: 'uppercase',
                  color: 'rgb(var(--c-ink-3))',
                }}
              >
                SSO corporativo
              </span>
            </div>

            <h1
              style={{
                fontFamily: 'var(--font-display)',
                fontWeight: 600,
                fontSize: '24px',
                letterSpacing: '-0.01em',
                margin: '0 0 6px',
                color: 'rgb(var(--c-ink))',
              }}
            >
              Concluindo seu acesso…
            </h1>
            <p
              style={{
                margin: 0,
                fontSize: '13.5px',
                color: 'rgb(var(--c-ink-3))',
                lineHeight: 1.6,
              }}
            >
              Confirmando sua identidade e abrindo o Portal da Infra.
            </p>
          </>
        )}
      </div>
    </DukkAuthShell>
  )
}
