import { useQueries } from "@tanstack/react-query";

import { getChange } from "../api/client";
import type { ChangeDetail } from "../api/types";

/** Fetch each change's detail (GET /api/changes/{id}) concurrently, keyed
 * ["change", id] so the fetches share react-query's cache across the dashboard
 * and the review page. Returns a Map by id of the details that have resolved
 * (pending/errored ids are simply absent). */
export function useChangeDetails(ids: number[]): Map<number, ChangeDetail> {
  const queries = useQueries({
    queries: ids.map((id) => ({
      queryKey: ["change", id],
      queryFn: () => getChange(id),
    })),
  });
  const byId = new Map<number, ChangeDetail>();
  ids.forEach((id, i) => {
    const detail = queries[i]?.data;
    if (detail) byId.set(id, detail);
  });
  return byId;
}
