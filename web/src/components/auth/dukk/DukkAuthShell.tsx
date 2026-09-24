// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * Moldura comum das telas de autenticação do Portal da Infra: login, handoff
 * de SSO e retorno do callback.
 *
 * Porte React do `LoginRedesignView.vue` do CRM. Ser uma moldura em vez de
 * três telas independentes é o que garante o pedido de "tudo no DS": a página
 * de SSO e a de callback não são variações aproximadas do login, são o mesmo
 * layout com outro conteúdo na coluna direita — então não há como uma delas
 * derivar visualmente sem que as três derivem juntas.
 *
 * O wrapper carrega a classe `.dukk-ds`, que é o escopo dos tokens `--c-*`
 * (ver `styles/dukk-ds.css`). O toggle sol/lua dirige o tema GLOBAL do app via
 * `next-themes`, igual ao resto do painel — e não um estado local, senão a
 * preferência se perderia ao sair do login.
 */

import { useTheme } from '@/components/providers/ThemeProvider'
import { Moon, Sun } from 'lucide-react'
import type { ReactNode } from 'react'

import { InfraVisualPanel } from './InfraVisualPanel'

export function DukkAuthShell({ children }: { children: ReactNode }) {
  const { resolvedTheme, setTheme } = useTheme()
  const isDark = resolvedTheme === 'dark'

  return (
    <div
      className="dukk-ds dukk-auth-shell"
      style={{
        minHeight: '100vh',
        color: 'rgb(var(--c-ink))',
        background: 'rgb(var(--c-canvas))',
        display: 'grid',
        gridTemplateColumns: 'minmax(0,1.05fr) minmax(420px,0.85fr)',
      }}
    >
      <InfraVisualPanel theme={isDark ? 'dark' : 'light'} />

      <div
        style={{
          position: 'relative',
          display: 'flex',
          flexDirection: 'column',
          background: 'rgb(var(--c-canvas))',
        }}
      >
        <header
          style={{
            display: 'flex',
            justifyContent: 'flex-end',
            padding: '24px 40px 0',
          }}
        >
          <button
            type="button"
            title="Alternar tema claro/escuro"
            aria-label="Alternar tema claro/escuro"
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              width: '36px',
              height: '36px',
              border: '1px solid rgb(var(--c-line))',
              background: 'rgb(var(--c-paper))',
              color: 'rgb(var(--c-ink))',
              borderRadius: '8px',
              cursor: 'pointer',
              flexShrink: 0,
            }}
            onClick={() => setTheme(isDark ? 'light' : 'dark')}
          >
            {isDark ? (
              <Moon width={16} height={16} color="currentColor" />
            ) : (
              <Sun width={17} height={17} color="currentColor" />
            )}
          </button>
        </header>

        <main
          style={{
            flex: 1,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            padding: '24px 40px 64px',
          }}
        >
          {children}
        </main>

        <p
          style={{
            textAlign: 'center',
            fontSize: '12px',
            color: 'rgb(var(--c-ink-3))',
            margin: '0 0 28px',
            lineHeight: 1.5,
          }}
        >
          Protegido por{' '}
          <strong style={{ color: 'rgb(var(--c-ink-2))', fontWeight: 600 }}>
            Dukk Enterprise
          </strong>{' '}
          · dúvidas de acesso, contate o administrador da sua empresa.
        </p>
      </div>
    </div>
  )
}
