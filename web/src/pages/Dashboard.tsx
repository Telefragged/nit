import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";
import { listChains, listRepos } from "../api/client";
import type { Chain } from "../api/types";
import { StateBadge, StatusDot, PartialBadge } from "../components/badges";
import { repoPath } from "../lib/repo";
import { timeAgo } from "../lib/time";
import { useRowNav } from "../lib/useRowNav";
import { ErrorPanel } from "./NotFound";

function ChainRow({ chain }: { chain: Chain }) {
  const rowNav = useRowNav(`/chains/${chain.id}`);
  return (
    <tr {...rowNav}>
      <td className="branch-cell">
        <div>
          <Link className="branch" to={`/chains/${chain.id}`}>
            {chain.branch}
          </Link>
          {chain.last_scan_error ? (
            <span className="error-glyph" title={chain.last_scan_error}>
              ✗ scan failed
            </span>
          ) : null}
        </div>
        <div className="repo">base {chain.base}</div>
      </td>
      <td>
        <div className="badge-group">
          <StateBadge state={chain.state} />
          {chain.partial ? <PartialBadge /> : null}
        </div>
      </td>
      <td>
        <div className="dots">
          {chain.changes.map((change) => (
            <Link key={change.id} to={`/changes/${change.id}`}>
              <StatusDot
                status={change.status}
                title={`${(change.position ?? 0) + 1}. ${change.subject} — ${change.status}`}
              />
            </Link>
          ))}
        </div>
      </td>
      <td className="time-cell">{timeAgo(chain.updated_at)}</td>
    </tr>
  );
}

function SkeletonRows() {
  return (
    <>
      {[0, 1, 2].map((i) => (
        <tr key={i}>
          <td>
            <div className="skeleton" style={{ width: 180 }} />
            <div className="skeleton" style={{ width: 110, marginTop: 6 }} />
          </td>
          <td>
            <div className="skeleton" style={{ width: 120 }} />
          </td>
          <td>
            <div className="skeleton" style={{ width: 60 }} />
          </td>
          <td>
            <div className="skeleton" style={{ width: 48 }} />
          </td>
        </tr>
      ))}
    </>
  );
}

export default function Dashboard() {
  const { repoId } = useParams();
  const id = Number(repoId);
  const reposQuery = useQuery({
    queryKey: ["repos"],
    queryFn: listRepos,
    refetchInterval: 5_000,
  });
  const query = useQuery({
    queryKey: ["chains", "active", id],
    queryFn: () => listChains("active", id),
    refetchInterval: 5_000,
  });

  const repo = reposQuery.data?.repos.find((r) => r.id === id);
  const gitDir = repo?.git_dir ?? query.data?.chains[0]?.git_dir;

  return (
    <main className="page">
      <h1 className="mono">{gitDir ? repoPath(gitDir) : "Repository"}</h1>
      <p className="subtitle">
        <Link to="/">← Repositories</Link> · active review chains in this repo.
      </p>
      {query.isError ? (
        <ErrorPanel error={query.error} />
      ) : (
        <table className="list">
          <thead>
            <tr>
              <th style={{ width: "40%" }}>Chain</th>
              <th style={{ width: 170 }}>State</th>
              <th>Changes</th>
              <th style={{ width: 90 }}>Updated</th>
            </tr>
          </thead>
          <tbody>
            {query.isPending ? (
              <SkeletonRows />
            ) : query.data.chains.length === 0 ? (
              <tr>
                <td colSpan={4}>
                  <div className="empty-state" style={{ border: "none" }}>
                    No active chains in this repo. Run <code>nit push</code>{" "}
                    from it to register one.
                  </div>
                </td>
              </tr>
            ) : (
              query.data.chains.map((chain) => (
                <ChainRow key={chain.id} chain={chain} />
              ))
            )}
          </tbody>
        </table>
      )}
    </main>
  );
}
