import { useQueries } from "@tanstack/react-query";

import { getChange } from "../api/client";
import type { ChangeDetail } from "../api/types";

/** Cache key ["change", id] shares react-query's cache across the dashboard
 * and review page. Pending/errored ids are absent from the returned Map. */
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
