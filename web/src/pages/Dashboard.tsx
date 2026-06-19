import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";
import { Link, useLocation, useParams } from "react-router-dom";
import { getRepoGraph, listRepos } from "../api/client";
import ChangeGraph from "../components/ChangeGraph";
import { repoPath } from "../lib/repo";
import { ErrorPanel } from "./NotFound";

/** A repo's review dashboard: one spine-centered change graph over the
 * canonical branch — open changes ascending above the HEAD anchor, merged
 * history descending below it (docs/api.md "Graph"). Replaces the per-chain
 * tables. */
export default function Dashboard() {
  const { repoId } = useParams();
  const id = Number(repoId);

  const reposQuery = useQuery({
    queryKey: ["repos"],
    queryFn: listRepos,
    refetchInterval: 5_000,
  });
  const graphQuery = useQuery({
    queryKey: ["graph", id],
    queryFn: () => getRepoGraph(id),
    refetchInterval: 5_000,
  });

  const repo = reposQuery.data?.repos.find((r) => r.id === id);

  // Restore the review breadcrumb's #chain-<tip> scroll: react-router doesn't
  // scroll to a fragment, and the target row only exists once the async graph
  // query resolves.
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
            over <span className="mono">{repo.base_branch}</span>
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
        <ChangeGraph graph={graphQuery.data} />
      )}
    </main>
  );
}
