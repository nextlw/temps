// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * What the login screen offers has to agree with what the server will accept.
 * The server's own rule is tested in `temps-auth`; these pin the screen that
 * reads its answer, including the two states that have no way in if the markup
 * is wrong: SSO-only with the password fields still drawn (offering a
 * credential that will be refused), and SSO-only with nothing drawn at all.
 */

import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { MemoryRouter } from 'react-router'

import { InfraLoginForm } from './InfraLoginForm'

const PROVIDER = { slug: 'zitadel-abc123', name: 'Zitadel', template: 'generic' }

function render(props: Parameters<typeof InfraLoginForm>[0]) {
  return renderToStaticMarkup(
    <MemoryRouter initialEntries={['/login']}>
      <InfraLoginForm {...props} />
    </MemoryRouter>
  )
}

const noop = async () => {}

describe('InfraLoginForm', () => {
  test('offers both ways in when the server accepts a password', () => {
    const markup = render({
      onSubmit: noop,
      oidcProviders: [PROVIDER],
      passwordLoginEnabled: true,
    })

    expect(markup).toContain('type="password"')
    expect(markup).toContain('E-mail corporativo')
    expect(markup).toContain('Continuar com SSO corporativo')
    // The "ou" divider only makes sense with two options to separate.
    expect(markup).toContain('ou</span>')
  })

  test('collapses to SSO alone when the server refuses passwords', () => {
    const markup = render({
      onSubmit: noop,
      oidcProviders: [PROVIDER],
      passwordLoginEnabled: false,
    })

    // The credential the server would refuse is never offered.
    expect(markup).not.toContain('type="password"')
    expect(markup).not.toContain('E-mail corporativo')
    // Nor a divider with nothing above it.
    expect(markup).not.toContain('ou</span>')

    expect(markup).toContain('Continuar com SSO corporativo')
    expect(markup).toContain('SSO corporativo da sua empresa')
  })

  /**
   * With the form gone, an outline button alone reads as secondary — or as
   * disabled. The only way in should look like the primary action, which in
   * this design system means the accent background.
   */
  test('the SSO button takes the primary weight when it is the only way in', () => {
    const asOnlyOption = render({
      onSubmit: noop,
      oidcProviders: [PROVIDER],
      passwordLoginEnabled: false,
    })
    expect(asOnlyOption).toContain('background:rgb(var(--c-lime))')

    const alongsidePassword = render({
      onSubmit: noop,
      oidcProviders: [PROVIDER],
      passwordLoginEnabled: true,
    })
    // Here the accent belongs to "Entrar"; the SSO button stays an outline.
    expect(alongsidePassword).toContain('border:1px solid rgb(var(--c-line))')
  })

  /**
   * Unreachable through the real endpoint — the server keeps password login on
   * while no provider is enabled, precisely so an instance always has a way in
   * — but a stale or truncated response would otherwise render a blank panel,
   * which an operator cannot tell apart from a portal that is down.
   */
  test('says so instead of rendering a dead end with no way in', () => {
    const markup = render({
      onSubmit: noop,
      oidcProviders: [],
      passwordLoginEnabled: false,
    })

    expect(markup).not.toContain('type="password"')
    expect(markup).not.toContain('Continuar com SSO corporativo')
    expect(markup).toContain('nenhum provedor de SSO está disponível')
    expect(markup).toContain('Contate o administrador')
  })

  test('the reset link only appears when a reset can actually be delivered', () => {
    const withEmail = render({
      onSubmit: noop,
      passwordLoginEnabled: true,
      passwordResetAvailable: true,
    })
    expect(withEmail).toContain('Esqueceu a senha?')

    const withoutEmail = render({
      onSubmit: noop,
      passwordLoginEnabled: true,
      passwordResetAvailable: false,
    })
    expect(withoutEmail).not.toContain('Esqueceu a senha?')
  })

  test('a failed SSO return is surfaced in the form error banner', () => {
    const markup = render({
      onSubmit: noop,
      oidcProviders: [PROVIDER],
      externalError: 'Este link de SSO expirou. Inicie o login novamente.',
    })

    expect(markup).toContain('role="alert"')
    expect(markup).toContain('Este link de SSO expirou')
  })
})
