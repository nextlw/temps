// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

const KEY = 'temps:returnTo'

/**
 * Paths that must never be captured as a post-login destination.
 *
 * `/login` matters most and is the least obvious: it is not a registered route
 * at all. It falls through to `/*` -> ProtectedLayout, which renders the login
 * form in place (the URL stays `/login`) and calls `captureReturnTo()`. Without
 * this entry, signing in at `/login` stored `/login` as the return target and
 * then navigated back to it -- now authenticated, where `AuthenticatedRoutes`
 * has no such route, so the user landed on "404 - Page Not Found" immediately
 * after a successful login. Bookmarking the login page is entirely normal, so
 * this was easy to hit and impossible to self-diagnose.
 *
 * The password-reset paths are here for the same reason: they are public routes
 * that make no sense to return to once a session exists.
 */
const AUTH_PATHS = new Set([
  '/mfa-verify',
  '/login',
  '/forgot-password',
  '/auth/reset-password',
  /*
   * Trajeto do SSO. `/auth/sso/callback` é o que mais importa: é o `return_to`
   * que a tela de handoff entrega ao servidor, então ele É a URL corrente
   * enquanto a própria tela chama `consumeReturnTo()`. Sem esta entrada, uma
   * captura feita ali guardaria o callback como destino e o login terminaria
   * voltando para a tela de "concluindo acesso" em loop.
   */
  '/auth/sso/callback',
])

/**
 * O handoff é `/auth/sso/{slug}`, com um segmento variável — não dá para
 * enumerá-lo no conjunto acima. Ele é tão inválido como destino pós-login
 * quanto o callback: voltar para lá reiniciaria o SSO de quem já entrou.
 */
const AUTH_PREFIXES = ['/auth/sso/']

function isAuthPath(path: string): boolean {
  const pathname = path.split('?')[0]?.split('#')[0] ?? path
  if (AUTH_PATHS.has(pathname)) return true
  return AUTH_PREFIXES.some((prefix) => pathname.startsWith(prefix))
}

export function captureReturnTo(): void {
  if (typeof window === 'undefined') return
  const current = `${window.location.pathname}${window.location.search}${window.location.hash}`
  if (!current || current === '/' || isAuthPath(current)) return
  try {
    window.sessionStorage.setItem(KEY, current)
  } catch {
    /* storage disabled */
  }
}

export function consumeReturnTo(fallback = '/dashboard'): string {
  if (typeof window === 'undefined') return fallback
  try {
    const value = window.sessionStorage.getItem(KEY)
    window.sessionStorage.removeItem(KEY)
    if (value && !isAuthPath(value)) return value
  } catch {
    /* storage disabled */
  }
  return fallback
}
