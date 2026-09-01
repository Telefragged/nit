import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { Link, useParams, useSearchParams } from "react-router-dom";
import { getChanges, getHistory, getRepo, getTags } from "../api/client";
import { repoGraph } from "../api/fold";
import type { ChangeStatus } from "../api/types";
import ChangeGraph, { type NodeActivity } from "../components/ChangeGraph";
import { repoPath } from "../lib/repo";
import { useDrafts } from "../lib/useDrafts";
import { ErrorPanel } from "./NotFound";

/** Every status but `merged`: the open region derives from active tips, but a
 * walk may pass through an abandoned member (abandonment is
 * membership-inert), so the fetch must resolve those too. */
const GRAPH_STATUSES: ChangeStatus[] = [
  "pending",
  "approved",
  "changes_requested",
  "commented",
  "abandoned",
];

/** A repo's review dashboard: one change graph centered on the canonical
 * ref — open changes ascending above the HEAD anchor, merged
 * history descending below it — assembled in the browser (api/fold) from the
 * repo's unmerged change folds and its canonical history. `?group=<key>`
 * groups the open changes by that tag key. */
export default function Dashboard() {
  const { repoId } = useParams();
  const id = Number(repoId);
  const [params, setParams] = useSearchParams();
  const groupBy = params.get("group");
  const setGroupBy = (key: string) => {
    const next = new URLSearchParams(params);
    if (key === "") next.delete("group");
    else next.set("group", key);
    setParams(next, { replace: true });
  };

  // The repo's path (its name) is fixed for the page's lifetime, so fetch it
  // once by id — only the changes/history reads refetch as things land.
  const repoQuery = useQuery({
    queryKey: ["repo", id],
    queryFn: () => getRepo(id),
  });
  const changesQuery = useQuery({
    queryKey: ["repo-changes", id],
    queryFn: () => getChanges(id, GRAPH_STATUSES),
  });
  const historyQuery = useQuery({
    queryKey: ["history", id],
    queryFn: () => getHistory(id),
  });
  // The keys the selector offers: those an unmerged change carries now,
  // plus the one the URL names, so a stale link still shows its choice.
  const tagsQuery = useQuery({
    queryKey: ["repo-tags", id],
    queryFn: () => getTags(id, GRAPH_STATUSES),
  });
  const groupKeys = useMemo(() => {
    const keys = Object.keys(tagsQuery.data?.tags ?? {});
    if (groupBy !== null && !keys.includes(groupBy)) keys.push(groupBy);
    return keys;
  }, [tagsQuery.data, groupBy]);
  const graph = useMemo(
    () =>
      changesQuery.data && historyQuery.data
        ? repoGraph(changesQuery.data.changes, historyQuery.data, groupBy)
        : undefined,
    [changesQuery.data, historyQuery.data, groupBy],
  );

  // Each open node's activity badges read straight off the fold the bulk
  // read already delivered. Only the reviewer's drafts and draft decision
  // are outside the log, so that overlay is the one per-change read
  // (GET /changes/{id}/drafts).
  const activityIds = useMemo(
    () =>
      (graph?.nodes ?? []).flatMap((n) =>
        n.section === "open" && n.change_number !== null
          ? [n.change_number]
          : [],
      ),
    [graph],
  );
  const overlays = useDrafts(activityIds);
  const projs = changesQuery.data?.changes;
  const activity = useMemo(() => {
    const projById = new Map((projs ?? []).map((p) => [p.id, p]));
    return new Map<number, NodeActivity>(
      activityIds.map((id) => [
        id,
        {
          threads: projById.get(id)?.threads ?? [],
          drafts: overlays.get(id)?.drafts ?? [],
          decision: overlays.get(id)?.draft_decision?.decision ?? null,
        },
      ]),
    );
  }, [activityIds, projs, overlays]);

  const repo = repoQuery.data;
  const error = changesQuery.error ?? historyQuery.error;

  return (
    <main className="page">
      <h1 className="mono">{repo ? repoPath(repo.git_dir) : "Repository"}</h1>
      <p className="subtitle">
        <Link to="/" className="mono">
          ← Repositories
        </Link>{" "}
        · change graph
        {repo ? (
          <>
            {" "}
            over <span className="mono">{repo.canonical_ref}</span>
          </>
        ) : null}
        .
      </p>
      {groupKeys.length > 0 && (
        <div className="graph-toolbar">
          <label>
            Group by
            <select
              className="revision-select"
              value={groupBy ?? ""}
              onChange={(e) => {
                setGroupBy(e.target.value);
              }}
            >
              <option value="">none</option>
              {groupKeys.map((key) => (
                <option key={key} value={key}>
                  {key}
                </option>
              ))}
            </select>
          </label>
        </div>
      )}
      {error ? (
        <ErrorPanel error={error} />
      ) : graph === undefined ? (
        <div className="skeleton" style={{ height: 320 }} />
      ) : graph.nodes.length === 0 ? (
        <div className="empty-state">
          Nothing here yet. Run <code>nit push</code> from this repo to register
          a change for review.
        </div>
      ) : (
        <ChangeGraph graph={graph} activity={activity} />
      )}
    </main>
  );
}
