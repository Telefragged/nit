import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { listRepos } from "../api/client";
import type { Repo } from "../api/types";
import { repoPath } from "../lib/repo";
import { useRowNav } from "../lib/useRowNav";
import { ErrorPanel } from "./NotFound";

function RepoRow({ repo }: { repo: Repo }) {
  const rowNav = useRowNav(`/repos/${repo.id}`);
  return (
    <tr {...rowNav}>
      <td className="branch-cell">
        <Link className="branch" to={`/repos/${repo.id}`}>
          {repoPath(repo.git_dir)}
        </Link>
        <div className="repo">base {repo.base_branch}</div>
      </td>
      <td className="count-cell">
        {repo.active_chains} active{" "}
        {repo.active_chains === 1 ? "chain" : "chains"}
      </td>
    </tr>
  );
}

function SkeletonRows() {
  return (
    <>
      {[0, 1].map((i) => (
        <tr key={i}>
          <td>
            <div className="skeleton" style={{ width: 240 }} />
            <div className="skeleton" style={{ width: 110, marginTop: 6 }} />
          </td>
          <td>
            <div className="skeleton" style={{ width: 70 }} />
          </td>
        </tr>
      ))}
    </>
  );
}

export default function RepoList() {
  const query = useQuery({
    queryKey: ["repos"],
    queryFn: listRepos,
  });

  return (
    <main className="page">
      <h1 className="mono">Repositories</h1>
      <p className="subtitle">
        Registered repositories — open one to review its chains.
      </p>
      {query.isError ? (
        <ErrorPanel error={query.error} />
      ) : (
        <table className="list">
          <thead>
            <tr>
              <th>Repository</th>
              <th style={{ width: 140 }}>Chains</th>
            </tr>
          </thead>
          <tbody>
            {query.isPending ? (
              <SkeletonRows />
            ) : query.data.repos.length === 0 ? (
              <tr>
                <td colSpan={2}>
                  <div className="empty-state" style={{ border: "none" }}>
                    No repositories. Run <code>nit push</code> from a repo to
                    register one.
                  </div>
                </td>
              </tr>
            ) : (
              query.data.repos.map((repo) => (
                <RepoRow key={repo.id} repo={repo} />
              ))
            )}
          </tbody>
        </table>
      )}
    </main>
  );
}
