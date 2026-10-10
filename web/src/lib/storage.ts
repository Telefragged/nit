/** Returns the JSON this browser saved under `key`, or `null` when there is
 * none or the browser blocks storage. */
export function readSaved(key: string): unknown {
  try {
    return JSON.parse(localStorage.getItem(key) ?? "null");
  } catch {
    return null;
  }
}

/** Saves `value` as JSON under `key`. A browser that blocks storage keeps
 * the value for the life of the page only. */
export function writeSaved(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // The caller already holds the value in React state.
  }
}
