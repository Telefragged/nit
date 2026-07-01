import { useQueries } from "@tanstack/react-query";

import { getChangeDrafts } from "../api/client";
import type { ChangeDrafts } from "../api/types";

/** The reviewer's overlay (drafts + staged decision) per change, REST-read from
 * GET /changes/{id}/drafts — separate from the websocket-folded ["change", id]
 * published projection, and refetched on the reviewer's own mutations. */
export function useDrafts(ids: number[]): Map<number, ChangeDrafts> {
  const queries = useQueries({
    queries: ids.map((id) => ({
      queryKey: ["drafts", id],
      queryFn: () => getChangeDrafts(id),
    })),
  });
  const byId = new Map<number, ChangeDrafts>();
  ids.forEach((id, i) => {
    const data = queries[i]?.data;
    if (data) byId.set(id, data);
  });
  return byId;
}
