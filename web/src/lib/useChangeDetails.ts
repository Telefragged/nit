import { skipToken, useQueries } from "@tanstack/react-query";

import { getChange } from "../api/client";
import type { ChangeDetail } from "../api/types";

/** Each change's published detail, keyed ["change", id]. By default fetches via
 * getChange — the dashboard's activity feed, where nothing else populates the
 * cache. With `cacheOnly`, reads the cache without fetching: the review page
 * folds each member off the websocket (useChangeStream), so a getChange would
 * re-fetch what the snapshot already delivers. Ids still loading are absent. */
export function useChangeDetails(
  ids: number[],
  cacheOnly = false,
): Map<number, ChangeDetail> {
  const queries = useQueries({
    queries: ids.map((id) => ({
      queryKey: ["change", id],
      queryFn: cacheOnly ? skipToken : () => getChange(id),
    })),
  });
  const byId = new Map<number, ChangeDetail>();
  ids.forEach((id, i) => {
    const detail = queries[i]?.data;
    if (detail) byId.set(id, detail);
  });
  return byId;
}
