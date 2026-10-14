import { useCallback } from "react";
import { useSearchParams } from "react-router-dom";

/** `prev` with `patch` applied. A `null` in the patch removes that key. */
export function patchParams(
  prev: URLSearchParams,
  patch: Record<string, string | null>,
): URLSearchParams {
  const next = new URLSearchParams(prev);
  for (const [key, value] of Object.entries(patch)) {
    if (value === null) next.delete(key);
    else next.set(key, value);
  }
  return next;
}

/** The page's query string, and a stable writer that patches it in place.
 *
 * The patch applies as `patchParams` applies it. The write replaces the history
 * entry, so a selector change adds no back-button stop. `state` becomes the
 * new location's state. */
export function useUrlParams(): [
  URLSearchParams,
  (patch: Record<string, string | null>, state?: unknown) => void,
] {
  const [params, setParams] = useSearchParams();
  const update = useCallback(
    (patch: Record<string, string | null>, state?: unknown) => {
      setParams((prev) => patchParams(prev, patch), { replace: true, state });
    },
    [setParams],
  );
  return [params, update];
}
