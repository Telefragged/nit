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
 * alt chord, typing in a text field, or any key while a dialog other than
 * a toast is open. The keyboard focus is in an open dialog, but a listener
 * on the window still gets each key. A toast is a dialog with
 * `aria-modal="false"`, and the focus stays on the page. The letter is
 * lowercased, and only the shift key adds the prefix `shift+`, so an `N`
 * typed with CapsLock is `n`. The review page and the review bar both use
 * this function, so their two keydown listeners accept the same
 * shortcuts. */
export function shortcutKey(e: KeyboardEvent): string | null {
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  if (document.querySelector('[role="dialog"]:not([aria-modal="false"])'))
    return null;
  const el = e.target as HTMLElement | null;
  if (el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return null;
  return (e.shiftKey ? "shift+" : "") + e.key.toLowerCase();
}
