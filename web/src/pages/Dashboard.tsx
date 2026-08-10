import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { Link, useParams } from "react-router-dom";
import { getChanges, getHistory, getRepo } from "../api/client";
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

/** A repo's review dashboard: one spine-centered change graph over the
 * canonical branch — open changes ascending above the HEAD anchor, merged
 * history descending below it — assembled in the browser (api/fold) from the
 * repo's unmerged change folds and its canonical history. */
export default function Dashboard() {
  const { repoId } = useParams();
  const id = Number(repoId);

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
  const graph = useMemo(
    () =>
      changesQuery.data && historyQuery.data
        ? repoGraph(changesQuery.data.changes, historyQuery.data)
        : undefined,
    [changesQuery.data, historyQuery.data],
  );

  // Each open node's activity badges read straight off the fold the bulk
  // read already delivered. Only the reviewer's drafts and draft decision
  // are outside the log, so that overlay is the one per-change read
  // (GET /changes/{id}/drafts).
  const activityIds = useMemo(
    () =>
      (graph?.nodes ?? []).flatMap((n) =>
        n.section === "open" && n.change_id !== null ? [n.change_id] : [],
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
