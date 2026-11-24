import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { getPorted } from "../api/client";
import type { ChangeVersion, Draft, ThreadProjection } from "../api/types";
import { anchorRevision, assembleThreads, portThreads } from "./comments";

/** The threads of a change as the reviewer sees them in the diff range
 * from `against` to `selected`.
 *
 * `threads` merges the published threads with the reviewer's drafts.
 * `shown` is `threads` with each thread of an earlier revision at its
 * ported place. Without `includeResolved`, a resolved thread does not
 * port, except the thread `linked`: the reviewer followed a link to it.
 * `ported` is the read of the ported places. */
export function useThreads({
  changeNumber,
  published,
  drafts,
  selected,
  against,
  includeResolved,
  linked,
  enabled,
}: {
  changeNumber: number;
  published: readonly ThreadProjection[];
  drafts: readonly Draft[];
  selected: number;
  against: ChangeVersion;
  includeResolved: boolean;
  linked: number | null;
  enabled: boolean;
}) {
  const threads = useMemo(
    () => assembleThreads(published, drafts),
    [published, drafts],
  );
  const portKey = useMemo(
    () =>
      threads
        .filter((t) => t.id !== null && anchorRevision(t.anchor) < selected)
        .map((t) => t.id)
        .join(","),
    [threads, selected],
  );
  const ported = useQuery({
    queryKey: ["ported", changeNumber, selected, against, portKey],
    queryFn: () => getPorted(changeNumber, selected, against),
    enabled: enabled && selected > 0,
  });
  const places = useMemo(() => {
    const all = ported.data ?? [];
    if (includeResolved) return all;
    const resolved = new Set(
      threads.filter((t) => t.resolved).map((t) => t.id),
    );
    return all.filter(
      (p) => p.thread_id === linked || !resolved.has(p.thread_id),
    );
  }, [ported.data, includeResolved, threads, linked]);
  const shown = useMemo(
    () => portThreads(threads, places, selected, against),
    [threads, places, selected, against],
  );
  return { threads, shown, ported };
}
