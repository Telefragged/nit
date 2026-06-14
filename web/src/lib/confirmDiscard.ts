/** Single confirmation point for every path that throws away editor text.
 * Returns true when discarding is OK: nothing dirty, or the user agreed.
 * `what` names the text being lost (the reply modal discards a reply). */
export function confirmDiscard(dirty: boolean, what = "comment"): boolean {
  return !dirty || window.confirm(`Discard unsaved ${what}?`);
}
