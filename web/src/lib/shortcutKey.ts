/** The shortcut a keypress stands for: `n`, `shift+n`, `[`, and so on.
 *
 * Returns null when the keypress drives no shortcut — a command, control or
 * alt chord, or typing in a text field. The letter is lowercased and the
 * shift is named, so CapsLock cannot pass a bare `n` off as `shift+n`.
 * Shared by the review page's diff/nav handler and the review bar's submit
 * handler so the two keydown listeners can't drift apart on what counts as
 * a shortcut. */
export function shortcutKey(e: KeyboardEvent): string | null {
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  const el = e.target as HTMLElement | null;
  if (el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return null;
  return (e.shiftKey ? "shift+" : "") + e.key.toLowerCase();
}
