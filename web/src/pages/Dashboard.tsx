import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { Link, useParams } from "react-router-dom";
import { getChanges, getHistory, getRepo } from "../api/client";
import { repoGraph } from "../api/fold";
import type { ChangeStatus } from "../api/types";
import ChangeGraph from "../components/ChangeGraph";
import { repoPath } from "../lib/repo";
import { useChangeDetails } from "../lib/useChangeDetails";
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
 * history descending below it — assembled in the browser (api/graph) from the
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

  // Each open node carries a change; fetch its detail concurrently so the
  // per-change activity (comment/draft counts, staged decision) is read from
  // GET /api/changes/{id} rather than denormalized onto the graph node. Keyed
  // ["change", id] so the fetch shares react-query's cache with the review
  // page — opening a change off the dashboard is then a warm read.
  const activityIds = useMemo(
    () =>
      (graph?.nodes ?? []).flatMap((n) =>
        n.section === "open" && n.change_id !== null ? [n.change_id] : [],
      ),
    [graph],
  );
  const activity = useChangeDetails(activityIds);

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
            over <span className="mono">{repo.base_ref}</span>
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
