import { useQuery } from "@tanstack/react-query";
import { NavLink, Outlet } from "react-router-dom";
import { listRepos } from "../api/client";
import type { Repo } from "../api/types";
import { repoPath } from "../lib/repo";
import { ErrorPanel } from "./NotFound";

function RepoLink({ repo }: { repo: Repo }) {
  const path = repoPath(repo.git_dir);
  return (
    <NavLink className="repo-link" to={`/repos/${repo.id}`} title={path}>
      <span className="repo-name">{path.slice(path.lastIndexOf("/") + 1)}</span>
      <span className="repo-open">
        {repo.open_changes} open{" "}
        {repo.open_changes === 1 ? "change" : "changes"}
      </span>
      <span className="repo-path">{path}</span>
    </NavLink>
  );
}

/** The repositories in a sidebar, beside the page of the child route. The
 * sidebar highlights the repository whose change graph is open. */
export default function Repos() {
  const query = useQuery({
    queryKey: ["repos"],
    queryFn: listRepos,
  });

  return (
    <div className="sidebar-layout repo-layout">
      <aside className="repo-sidebar" aria-busy={query.isLoading}>
        <div className="rail-title">Repositories</div>
        {query.isError ? (
          <ErrorPanel error={query.error} />
        ) : query.isPending ? (
          <div className="skeleton" style={{ height: 36 }} />
        ) : query.data.repos.length === 0 ? (
          <div className="empty-state">
            No repositories. Run <code>nit push</code> from a repo to register
            one.
          </div>
        ) : (
          query.data.repos.map((repo) => <RepoLink key={repo.id} repo={repo} />)
        )}
      </aside>
      <Outlet />
    </div>
  );
}

/** The page beside the sidebar before the reviewer picks a repository. */
export function NoRepo() {
  return (
    <main className="page">
      <div className="empty-state">
        Pick a repository to see its change graph.
      </div>
    </main>
  );
}
