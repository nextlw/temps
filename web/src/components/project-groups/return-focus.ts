// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import type { RefObject } from 'react'

/**
 * `onCloseAutoFocus` for a dialog opened by a button that is not its Radix
 * trigger (one dialog, several openers): focus goes back to the first ref
 * still in the page (WCAG 2.4.3), else Radix's default applies.
 */
export function returnFocusTo(
  event: Event,
  ...targets: RefObject<HTMLElement | null>[]
): void {
  const target = targets
    .map((ref) => ref.current)
    .find((element) => element?.isConnected)
  if (!target) return
  event.preventDefault()
  target.focus()
}
