/** Whether a click is the plain click that the page handles itself: the
 * main button, with no command, control or alt key. Shift is left to the
 * caller. The browser handles any other click, for example a ctrl-click
 * that opens a new tab. */
export const plainClick = (e: {
  button: number;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
}) => e.button === 0 && !e.ctrlKey && !e.metaKey && !e.altKey;

/** The shortcut a keypress stands for: `n`, `shift+n`, `[`, and so on.
 *
 * Returns null when the keypress is not a shortcut: a command, control or
 * alt chord, typing in a text field, or any key while a dialog is open. The
 * keyboard focus is in an open dialog, but a listener on the window still
 * gets each key. The letter is lowercased and the
 * shift is named, so CapsLock cannot pass a bare `n` off as `shift+n`.
 * Shared by the review page's diff/nav handler and the review bar's submit
 * handler so the two keydown listeners can't drift apart on what counts as
 * a shortcut. */
export function shortcutKey(e: KeyboardEvent): string | null {
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  if (document.querySelector('[role="dialog"]')) return null;
  const el = e.target as HTMLElement | null;
  if (el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return null;
  return (e.shiftKey ? "shift+" : "") + e.key.toLowerCase();
}
