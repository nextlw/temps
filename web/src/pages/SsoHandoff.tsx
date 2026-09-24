// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * Tela de handoff do SSO — o momento entre clicar em "Continuar com SSO
 * corporativo" e chegar no provedor de identidade.
 *
 * Existe porque o início do SSO é uma navegação de página inteira para
 * `/api/auth/oidc/login/{slug}`, que o servidor responde com um 302 para o
 * IdP. Sem esta tela o operador vê um branco entre o painel e o Zitadel — que
 * é justamente o pedaço do trajeto que não estava no design system.
 *
 * Ela também é o lugar onde o `return_to` é definido: mandamos o servidor
 * devolver o navegador em `/auth/sso/callback` (uma rota deste SPA) em vez do
 * destino final, para que o retorno também caia numa tela do DS. O destino
 * original não viaja na URL — ele fica no `sessionStorage` (`return-to.ts`),
 * que sobrevive à ida e volta ao IdP na mesma aba e não vira um parâmetro de
 * redirecionamento que alguém possa forjar.
 */

import { listPublicProvidersOptions } from '@/api/client/@tanstack/react-query.gen'
import { DukkAuthShell } from '@/components/auth/dukk/DukkAuthShell'
import { usePageTitle } from '@/hooks/usePageTitle'
import { useQuery } from '@tanstack/react-query'
import { Loader2 } from 'lucide-react'
import { useEffect, useMemo } from 'react'
import { useParams } from 'react-router'

/** Rota deste SPA onde o servidor devolve o navegador após o callback. */
const SSO_RETURN_PATH = '/auth/sso/callback'

/**
 * Uma pausa curta antes de sair: sem ela a tela pisca e não comunica nada, e
 * com muito mais o operador acha que travou. O suficiente para o texto ser
 * lido, não o bastante para irritar.
 */
const HANDOFF_DELAY_MS = 500

export const SsoHandoff = () => {
  usePageTitle('Entrando com SSO')
  const { slug = '' } = useParams<{ slug: string }>()

  const { data } = useQuery(listPublicProvidersOptions())

  const providerName = useMemo(() => {
    const match = data?.providers?.find((p) => p.slug === slug)
    return match?.name ?? 'provedor de identidade'
  }, [data, slug])

  useEffect(() => {
    if (!slug) return
    const target = `/api/auth/oidc/login/${encodeURIComponent(slug)}?return_to=${encodeURIComponent(SSO_RETURN_PATH)}`
    const timer = window.setTimeout(() => {
      window.location.assign(target)
    }, HANDOFF_DELAY_MS)
    return () => window.clearTimeout(timer)
  }, [slug])

  return (
    <DukkAuthShell>
      <div
        className="dukk-fadeup"
        style={{ width: '100%', maxWidth: '400px' }}
        aria-live="polite"
      >
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
          Levando você ao {providerName}…
        </h1>
        <p
          style={{
            margin: '0 0 24px',
            fontSize: '13.5px',
            color: 'rgb(var(--c-ink-3))',
            lineHeight: 1.6,
          }}
        >
          Você vai autenticar no provedor da sua empresa e voltar para cá em
          seguida. Sua senha não passa por este portal.
        </p>

        <p
          style={{
            margin: 0,
            fontSize: '12.5px',
            color: 'rgb(var(--c-ink-3))',
          }}
        >
          Não foi redirecionado?{' '}
          <a
            href={`/api/auth/oidc/login/${encodeURIComponent(slug)}?return_to=${encodeURIComponent(SSO_RETURN_PATH)}`}
            style={{
              color: 'rgb(var(--c-lime-fg))',
              fontWeight: 600,
              textDecoration: 'none',
            }}
          >
            continuar manualmente
          </a>
          .
        </p>
      </div>
    </DukkAuthShell>
  )
}
