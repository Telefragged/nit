/** True for a bare keypress that should drive a page shortcut: no modifier
 * held, and focus is not in a text field (so typing never fires one). Shared
 * by the review page's diff/nav handler and the review bar's submit handler so
 * the two keydown listeners can't drift apart on what counts as a shortcut. */
export function isShortcutKey(e: KeyboardEvent): boolean {
  if (e.metaKey || e.ctrlKey || e.altKey) return false;
  const el = e.target as HTMLElement | null;
  return !(el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
}
