import { queryOptions, useQueries } from "@tanstack/react-query";

import { getChangeDrafts } from "../api/client";
import type { ChangeDrafts } from "../api/types";

/** The read of the reviewer's overlay of change `id`, from
 * GET /changes/{id}/drafts. */
export const draftsQuery = (id: number) =>
  queryOptions({
    queryKey: ["drafts", id],
    queryFn: () => getChangeDrafts(id),
  });

/** The reviewer's overlay (drafts + draft decision) per change, REST-read from
 * GET /changes/{id}/drafts — separate from the websocket-folded ["change", id]
 * published projection, and refetched on the reviewer's own mutations.
 * `loading` holds while one of those reads has no answer yet. */
export function useDrafts(ids: number[]): {
  overlays: Map<number, ChangeDrafts>;
  loading: boolean;
} {
  const queries = useQueries({
    queries: ids.map(draftsQuery),
  });
  const overlays = new Map<number, ChangeDrafts>();
  ids.forEach((id, i) => {
    const data = queries[i]?.data;
    if (data) overlays.set(id, data);
  });
  return { overlays, loading: queries.some((q) => q.isLoading) };
}
