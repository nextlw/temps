// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * Formulário de login (coluna direita) do Portal da Infra.
 *
 * Porte React do `LoginForm.vue` do CRM: mesma hierarquia, mesmos tamanhos,
 * mesmas mensagens em português, mesmo botão de SSO abaixo do divisor "ou".
 *
 * Diferença deliberada em relação ao CRM: lá os handlers são stubs; aqui eles
 * ligam no fluxo real do Temps. O submit chama a mutation de login do painel
 * (que já trata MFA, troca de senha obrigatória e `return_to`), e o SSO passa
 * pela tela de handoff em vez de navegar direto para `/api/auth/oidc/login`.
 *
 * Também mantém o campo "manter conectado" apenas como affordance visual: o
 * `POST /api/auth/login` do Temps não aceita esse parâmetro, e o tempo de vida
 * do cookie de sessão é decidido pelo servidor. Enviar um `remember` que o
 * backend ignora daria ao operador a impressão de controlar algo que ele não
 * controla, então o checkbox é apresentado como o que é — sem prometer efeito.
 */

import { AlertCircle, AppWindow, Eye, EyeOff, Loader2 } from 'lucide-react'
import { useState } from 'react'
import { useNavigate } from 'react-router'

import type { OidcProviderOption } from '@/components/auth/login-form'

interface InfraLoginFormProps {
  onSubmit: (data: { email: string; password: string }) => Promise<void>
  isLoading?: boolean
  oidcProviders?: OidcProviderOption[]
  /**
   * Mensagem de erro vinda de fora — hoje o retorno de um SSO que falhou
   * (`/login?error=oidc_failed&reason=...`). Aparece no mesmo banner dos erros
   * do formulário para o operador não ter dois lugares onde olhar.
   */
  externalError?: string | null
  passwordResetAvailable?: boolean
}

export function InfraLoginForm({
  onSubmit,
  isLoading = false,
  oidcProviders = [],
  externalError = null,
  passwordResetAvailable = false,
}: InfraLoginFormProps) {
  const navigate = useNavigate()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [remember, setRemember] = useState(true)
  const [passwordVisible, setPasswordVisible] = useState(false)
  const [formError, setFormError] = useState('')

  const errorMessage = formError || externalError || ''
  const provider = oidcProviders[0]

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (isLoading) return
    if (!email || !password) {
      setFormError('Informe e-mail e senha para continuar.')
      return
    }
    if (!email.includes('@')) {
      setFormError('Digite um e-mail corporativo válido.')
      return
    }
    setFormError('')
    // O tratamento de falha (toast + título) fica na mutation da página, que é
    // quem conhece os códigos de erro do servidor.
    await onSubmit({ email: email.trim(), password })
  }

  const inputStyle: React.CSSProperties = {
    width: '100%',
    boxSizing: 'border-box',
    fontFamily: 'var(--font-body)',
    fontSize: '14.5px',
    padding: '11px 13px',
    borderRadius: '8px',
    border: '1px solid rgb(var(--c-line))',
    background: 'rgb(var(--c-paper))',
    color: 'rgb(var(--c-ink))',
  }

  const labelStyle: React.CSSProperties = {
    display: 'block',
    fontSize: '12px',
    fontWeight: 600,
    color: 'rgb(var(--c-ink-2))',
    marginBottom: '6px',
  }

  return (
    <div
      className="dukk-fadeup"
      style={{ width: '100%', maxWidth: '400px' }}
    >
      <div style={{ marginBottom: '28px' }}>
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
          Entrar no Portal da Infra
        </h1>
        <p
          style={{
            margin: 0,
            fontSize: '13.5px',
            color: 'rgb(var(--c-ink-3))',
          }}
        >
          Use as credenciais fornecidas pela sua empresa ou continue com o SSO
          corporativo.
        </p>
      </div>

      {errorMessage && (
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
            {errorMessage}
          </span>
        </div>
      )}

      <form onSubmit={handleSubmit}>
        <div style={{ marginBottom: '16px' }}>
          <label htmlFor="dukk-login-email" style={labelStyle}>
            E-mail corporativo
          </label>
          <input
            id="dukk-login-email"
            type="email"
            value={email}
            placeholder="voce@suaempresa.com"
            autoComplete="username"
            disabled={isLoading}
            style={inputStyle}
            onChange={(e) => {
              setEmail(e.target.value)
              setFormError('')
            }}
          />
        </div>

        <div style={{ marginBottom: '10px' }}>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              marginBottom: '6px',
            }}
          >
            <label
              htmlFor="dukk-login-password"
              style={{
                fontSize: '12px',
                fontWeight: 600,
                color: 'rgb(var(--c-ink-2))',
              }}
            >
              Senha
            </label>
            {passwordResetAvailable && (
              <button
                type="button"
                style={{
                  border: 0,
                  background: 'transparent',
                  padding: 0,
                  fontSize: '12px',
                  fontWeight: 600,
                  color: 'rgb(var(--c-lime-fg))',
                  cursor: 'pointer',
                  fontFamily: 'var(--font-body)',
                }}
                onClick={() => navigate('/forgot-password')}
              >
                Esqueceu a senha?
              </button>
            )}
          </div>
          <div style={{ position: 'relative' }}>
            <input
              id="dukk-login-password"
              type={passwordVisible ? 'text' : 'password'}
              value={password}
              placeholder="••••••••"
              autoComplete="current-password"
              disabled={isLoading}
              style={{ ...inputStyle, padding: '11px 40px 11px 13px' }}
              onChange={(e) => {
                setPassword(e.target.value)
                setFormError('')
              }}
            />
            <button
              type="button"
              title="Mostrar/ocultar senha"
              aria-label="Mostrar/ocultar senha"
              style={{
                position: 'absolute',
                right: '6px',
                top: '50%',
                transform: 'translateY(-50%)',
                border: 0,
                background: 'transparent',
                color: 'rgb(var(--c-ink-3))',
                cursor: 'pointer',
                padding: '6px',
                display: 'grid',
                placeItems: 'center',
                borderRadius: '6px',
              }}
              onClick={() => setPasswordVisible((v) => !v)}
            >
              {passwordVisible ? (
                <EyeOff width={16} height={16} color="currentColor" />
              ) : (
                <Eye width={16} height={16} color="currentColor" />
              )}
            </button>
          </div>
        </div>

        <label
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: '8px',
            margin: '14px 0 22px',
            cursor: 'pointer',
          }}
        >
          <input
            type="checkbox"
            checked={remember}
            style={{
              width: '15px',
              height: '15px',
              accentColor: 'rgb(var(--c-lime))',
              cursor: 'pointer',
            }}
            onChange={(e) => setRemember(e.target.checked)}
          />
          <span style={{ fontSize: '13px', color: 'rgb(var(--c-ink-2))' }}>
            Manter conectado neste dispositivo
          </span>
        </label>

        <button
          type="submit"
          disabled={isLoading}
          style={{
            width: '100%',
            boxSizing: 'border-box',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            gap: '8px',
            border: 0,
            background: 'rgb(var(--c-lime))',
            color: 'rgb(var(--c-on-lime))',
            padding: '12px 16px',
            borderRadius: '8px',
            fontFamily: 'var(--font-body)',
            fontSize: '14.5px',
            fontWeight: 700,
            cursor: isLoading ? 'default' : 'pointer',
            opacity: isLoading ? 0.7 : 1,
          }}
        >
          {isLoading && (
            <Loader2
              className="dukk-spin"
              width={16}
              height={16}
              color="currentColor"
              aria-hidden="true"
            />
          )}
          {isLoading ? 'Entrando…' : 'Entrar'}
        </button>
      </form>

      {provider && (
        <>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: '12px',
              margin: '24px 0',
            }}
          >
            <div
              style={{
                flex: 1,
                height: '1px',
                background: 'rgb(var(--c-line))',
              }}
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
              ou
            </span>
            <div
              style={{
                flex: 1,
                height: '1px',
                background: 'rgb(var(--c-line))',
              }}
            />
          </div>

          <button
            type="button"
            disabled={isLoading}
            style={{
              width: '100%',
              boxSizing: 'border-box',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              gap: '9px',
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
            onClick={() =>
              navigate(`/auth/sso/${encodeURIComponent(provider.slug)}`)
            }
          >
            <AppWindow width={16} height={16} color="currentColor" aria-hidden="true" />
            Continuar com SSO corporativo
          </button>
        </>
      )}

      {/*
        Mais de um provedor configurado: o desenho do CRM tem um único botão de
        SSO, então os demais entram como uma lista discreta abaixo em vez de
        empilhar botões iguais e desfigurar a tela.
      */}
      {oidcProviders.length > 1 && (
        <div
          style={{
            display: 'flex',
            flexWrap: 'wrap',
            justifyContent: 'center',
            gap: '8px',
            marginTop: '14px',
          }}
        >
          {oidcProviders.slice(1).map((other) => (
            <button
              key={other.slug}
              type="button"
              disabled={isLoading}
              style={{
                border: 0,
                background: 'transparent',
                padding: '4px 6px',
                fontSize: '12.5px',
                fontWeight: 600,
                color: 'rgb(var(--c-lime-fg))',
                cursor: 'pointer',
                fontFamily: 'var(--font-body)',
              }}
              onClick={() =>
                navigate(`/auth/sso/${encodeURIComponent(other.slug)}`)
              }
            >
              Entrar com {other.name}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
