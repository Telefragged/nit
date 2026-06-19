import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Link, useLocation, useParams } from "react-router-dom";
import { listChains, listRepos } from "../api/client";
import type { ChainSummary, PathEntry } from "../api/types";
import {
  NewerElsewhereBadge,
  PartialBadge,
  StateBadge,
  StatusChip,
  StatusDot,
} from "../components/badges";
import { repoPath } from "../lib/repo";
import { timeAgo } from "../lib/time";
import { useRowNav } from "../lib/useRowNav";
import { ErrorPanel } from "./NotFound";

/** One change in an expanded chain drawer — a member of the derived path,
 * read at the revision the path pins. */
function ChangeRow({ member }: { member: PathEntry }) {
  const rowNav = useRowNav(`/changes/${member.change_id}`);
  const { counts } = member;
  return (
    <tr {...rowNav}>
      <td className="pos-cell mono">{member.position}</td>
      <td className="subject-cell">
        <Link to={`/changes/${member.change_id}`} className="subject">
          {member.subject}
        </Link>
        <div className="meta">
          <span className="mono sha">{member.short_sha}</span>
          {member.newer_elsewhere ? (
            <NewerElsewhereBadge
              revision={member.revision}
              latest={member.latest_revision}
            />
          ) : null}
          {member.merged_elsewhere ? (
            <span
              className="badge badge-gray"
              title="A newer revision of this change landed on the canonical branch"
            >
              MERGED ELSEWHERE
            </span>
          ) : null}
        </div>
      </td>
      <td>
        <StatusChip status={member.status} />
      </td>
      <td className="count-cell mono">r{member.revision}</td>
      <td className="count-cell">
        <span className="counts">
          {counts.threads > 0 && (
            <span title="published comments">
              {counts.threads} comment
              {counts.threads > 1 ? "s" : ""}
            </span>
          )}
          {counts.drafts > 0 && (
            <span className="draft-count" title="your drafts">
              {counts.drafts} draft{counts.drafts > 1 ? "s" : ""}
            </span>
          )}
          {counts.unresolved > 0 && (
            <span className="unresolved-count" title="unresolved threads">
              {counts.unresolved} open
            </span>
          )}
        </span>
      </td>
    </tr>
  );
}

/** A chain as a collapsible drawer: a summary header (name, state, a
 * status-dot preview of the path, updated time) that expands in place to the
 * chain's changes — the dashboard drills into a chain without leaving the
 * repo page. Opens by default when deep-linked (`#chain-<tip>`). */
function ChainDrawer({
  chain,
  defaultOpen,
}: {
  chain: ChainSummary;
  defaultOpen: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const ref = useRef<HTMLElement>(null);

  // A deep-linked drawer scrolls itself into view once it mounts open.
  useEffect(() => {
    if (defaultOpen) ref.current?.scrollIntoView({ block: "start" });
  }, [defaultOpen]);

  return (
    <section
      className="chain-drawer"
      id={`chain-${chain.tip_change_id}`}
      ref={ref}
    >
      <button
        type="button"
        className="chain-drawer-head"
        aria-expanded={open}
        onClick={() => {
          setOpen((v) => !v);
        }}
      >
        <span className="fchevron">{open ? "▾" : "▸"}</span>
        <span className="chain-name mono">{chain.name}</span>
        <span className="badge-group">
          <StateBadge state={chain.state} />
          {chain.partial ? <PartialBadge /> : null}
        </span>
        <span className="spacer" />
        <span className="dots">
          {chain.path.map((member) => (
            <StatusDot
              key={member.change_id}
              status={member.status}
              title={`${member.position}. ${member.subject} — ${member.status}`}
            />
          ))}
        </span>
        <span className="time-cell">{timeAgo(chain.updated_at)}</span>
      </button>
      {open ? (
        <table className="list changes-table">
          <thead>
            <tr>
              <th style={{ width: 28 }}>#</th>
              <th>Change</th>
              <th style={{ width: 150 }}>Status</th>
              <th style={{ width: 80 }}>Rev</th>
              <th style={{ width: 150 }}>Activity</th>
            </tr>
          </thead>
          <tbody>
            {chain.path.map((member) => (
              <ChangeRow key={member.change_id} member={member} />
            ))}
          </tbody>
        </table>
      ) : null}
    </section>
  );
}

function SkeletonDrawers() {
  return (
    <>
      {[0, 1, 2].map((i) => (
        <div key={i} className="skeleton" style={{ height: 39 }} />
      ))}
    </>
  );
}

export default function Dashboard() {
  const { repoId } = useParams();
  const id = Number(repoId);
  const { hash } = useLocation();
  const openTip = hash.startsWith("#chain-") ? Number(hash.slice(7)) : null;

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
        <div className="chain-list">
          {query.isPending ? (
            <SkeletonDrawers />
          ) : query.data.chains.length === 0 ? (
            <div className="empty-state">
              No active chains in this repo. Run <code>nit push</code> from it
              to register one.
            </div>
          ) : (
            query.data.chains.map((chain) => (
              <ChainDrawer
                key={chain.tip_change_id}
                chain={chain}
                defaultOpen={chain.tip_change_id === openTip}
              />
            ))
          )}
        </div>
      )}
    </main>
  );
}
