import { useCallback } from "react";
import { useSearchParams } from "react-router-dom";

/** The page's query string, and a stable writer that patches it in place.
 *
 * A `null` in the patch removes that key. The write replaces the history
 * entry, so a selector change adds no back-button stop. */
export function useUrlParams(): [
  URLSearchParams,
  (patch: Record<string, string | null>) => void,
] {
  const [params, setParams] = useSearchParams();
  const update = useCallback(
    (patch: Record<string, string | null>) => {
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          for (const [key, value] of Object.entries(patch)) {
            if (value === null) next.delete(key);
            else next.set(key, value);
          }
          return next;
        },
        { replace: true },
      );
    },
    [setParams],
  );
  return [params, update];
}
