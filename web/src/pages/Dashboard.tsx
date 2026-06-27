import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo } from "react";
import { Link, useLocation, useParams } from "react-router-dom";
import { getRepo, getRepoGraph } from "../api/client";
import ChangeGraph from "../components/ChangeGraph";
import { repoPath } from "../lib/repo";
import { useChangeDetails } from "../lib/useChangeDetails";
import { ErrorPanel } from "./NotFound";

/** A repo's review dashboard: one spine-centered change graph over the
 * canonical branch — open changes ascending above the HEAD anchor, merged
 * history descending below it (docs/api.md "Graph"). */
export default function Dashboard() {
  const { repoId } = useParams();
  const id = Number(repoId);

  // The repo's path (its name) is fixed for the page's lifetime, so fetch it
  // once by id — only the graph polls for changes as they land.
  const repoQuery = useQuery({
    queryKey: ["repo", id],
    queryFn: () => getRepo(id),
  });
  const graphQuery = useQuery({
    queryKey: ["graph", id],
    queryFn: () => getRepoGraph(id),
  });

  // Each open node carries a change; fetch its detail concurrently so the
  // per-change activity (comment/draft counts, staged decision) is read from
  // GET /api/changes/{id} rather than denormalized onto the graph node. Keyed
  // ["change", id] so the fetch shares react-query's cache with the review
  // page — opening a change off the dashboard is then a warm read.
  const activityIds = useMemo(
    () =>
      (graphQuery.data?.nodes ?? []).flatMap((n) =>
        n.section === "open" && n.change_id !== null ? [n.change_id] : [],
      ),
    [graphQuery.data],
  );
  const activity = useChangeDetails(activityIds);

  const repo = repoQuery.data;

  // Honor the post-submit navigate's #chain-<tip> scroll (ReviewBar lands here
  // after publishing a chain's review): react-router doesn't scroll to a
  // fragment, and the target row only exists once the async graph query resolves.
  const { hash } = useLocation();
  useEffect(() => {
    if (!hash || !graphQuery.data) return;
    document.getElementById(hash.slice(1))?.scrollIntoView({ block: "start" });
  }, [hash, graphQuery.data]);

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
      {graphQuery.isError ? (
        <ErrorPanel error={graphQuery.error} />
      ) : graphQuery.isPending ? (
        <div className="skeleton" style={{ height: 320 }} />
      ) : graphQuery.data.nodes.length === 0 ? (
        <div className="empty-state">
          Nothing here yet. Run <code>nit push</code> from this repo to register
          a change for review.
        </div>
      ) : (
        <ChangeGraph graph={graphQuery.data} activity={activity} />
      )}
    </main>
  );
}
