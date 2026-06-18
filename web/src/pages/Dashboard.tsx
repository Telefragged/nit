import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";
import { listChains, listRepos } from "../api/client";
import type { ChainSummary } from "../api/types";
import { StateBadge, StatusDot, PartialBadge } from "../components/badges";
import { repoPath } from "../lib/repo";
import { timeAgo } from "../lib/time";
import { useRowNav } from "../lib/useRowNav";
import { ErrorPanel } from "./NotFound";

function ChainRow({ chain }: { chain: ChainSummary }) {
  const rowNav = useRowNav(`/chains/${chain.tip_change_id}`);
  return (
    <tr {...rowNav}>
      <td className="branch-cell">
        <Link className="branch" to={`/chains/${chain.tip_change_id}`}>
          {chain.name}
        </Link>
      </td>
      <td>
        <div className="badge-group">
          <StateBadge state={chain.state} />
          {chain.partial ? <PartialBadge /> : null}
        </div>
      </td>
      <td>
        <div className="dots">
          {chain.path.map((member) => (
            <Link key={member.change_id} to={`/changes/${member.change_id}`}>
              <StatusDot
                status={member.status}
                title={`${member.position + 1}. ${member.subject} — ${member.status}`}
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

  return (
    <main className="page">
      <h1 className="mono">{repo ? repoPath(repo.git_dir) : "Repository"}</h1>
      <p className="subtitle">
        <Link to="/" className="mono">
          ← Repositories
        </Link>{" "}
        · active review chains
        {repo ? (
          <>
            {" "}
            over <span className="mono">{repo.base_branch}</span>
          </>
        ) : null}
        .
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
                <ChainRow key={chain.tip_change_id} chain={chain} />
              ))
            )}
          </tbody>
        </table>
      )}
    </main>
  );
}
