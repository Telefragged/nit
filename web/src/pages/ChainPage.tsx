import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";
import { getChain } from "../api/client";
import type { PathEntry } from "../api/types";
import {
  StateBadge,
  StatusChip,
  PartialBadge,
  NewerElsewhereBadge,
} from "../components/badges";
import { useRowNav } from "../lib/useRowNav";
import { ErrorPanel } from "./NotFound";

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

export default function ChainPage() {
  const { id } = useParams();
  const tipChangeId = Number(id);
  const query = useQuery({
    queryKey: ["chain", tipChangeId],
    queryFn: () => getChain(tipChangeId),
    refetchInterval: 5_000,
  });

  if (query.isError) {
    return (
      <main className="page">
        <ErrorPanel error={query.error} />
      </main>
    );
  }
  if (query.isPending) {
    return (
      <main className="page">
        <div className="skeleton" style={{ width: 260, height: 18 }} />
        <div className="skeleton" style={{ width: 180, marginTop: 10 }} />
        <div className="skeleton" style={{ marginTop: 24, height: 90 }} />
      </main>
    );
  }

  const chain = query.data;

  return (
    <main className="page">
      <div className="chain-header">
        <h1 className="mono">{chain.name}</h1>
        <StateBadge state={chain.state} />
        {chain.partial ? <PartialBadge /> : null}
      </div>
      <p className="subtitle">
        <Link to={`/repos/${chain.repo_id}`} className="mono">
          ← Repository
        </Link>{" "}
        · base <span className="mono">{chain.base_branch}</span>
      </p>

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
          {chain.path.length === 0 ? (
            <tr>
              <td colSpan={5}>
                <div className="empty-state" style={{ border: "none" }}>
                  Chain is empty — the branch has no commits over{" "}
                  <code>{chain.base_branch}</code>.
                </div>
              </td>
            </tr>
          ) : (
            chain.path.map((member) => (
              <ChangeRow key={member.change_id} member={member} />
            ))
          )}
        </tbody>
      </table>
    </main>
  );
}
