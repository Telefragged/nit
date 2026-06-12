import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";
import { getChain } from "../api/client";
import type { ChangeSummary } from "../api/types";
import { StateBadge, StatusChip, PartialBadge } from "../components/badges";
import { timeAgo } from "../lib/time";
import { useRowNav } from "../lib/useRowNav";
import { ErrorPanel } from "./NotFound";

function ChangeRow({ change }: { change: ChangeSummary }) {
  const rowNav = useRowNav(`/changes/${change.id}`);
  const { counts } = change;
  return (
    <tr {...rowNav}>
      <td className="pos-cell mono">
        {change.position !== null ? change.position + 1 : "—"}
      </td>
      <td className="subject-cell">
        <Link to={`/changes/${change.id}`} className="subject">
          {change.subject}
        </Link>
        <div className="meta">
          <span className="mono sha">{change.short_sha}</span>
          {change.last_reviewed_revision !== null &&
          change.last_reviewed_revision < change.revision ? (
            <span
              className="badge badge-amber"
              title={`New revision since your last review (r${change.last_reviewed_revision} → r${change.revision})`}
            >
              UPDATED SINCE YOUR REVIEW ({change.last_reviewed_revision}→
              {change.revision})
            </span>
          ) : null}
        </div>
      </td>
      <td>
        <StatusChip status={change.status} />
      </td>
      <td className="count-cell mono">
        r{change.revision}
        {counts.revisions > 1 ? (
          <span className="dim"> of {counts.revisions}</span>
        ) : null}
      </td>
      <td className="count-cell">
        <span className="counts">
          {counts.published_comments > 0 && (
            <span title="published comments">
              {counts.published_comments} comment
              {counts.published_comments > 1 ? "s" : ""}
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
  const chainId = Number(id);
  const query = useQuery({
    queryKey: ["chain", chainId],
    queryFn: () => getChain(chainId),
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
  const live = chain.changes.filter((c) => c.status !== "orphaned");
  const orphaned = chain.changes.filter((c) => c.status === "orphaned");

  return (
    <main className="page">
      <div className="chain-header">
        <h1 className="mono">{chain.branch}</h1>
        <StateBadge state={chain.state} />
        {chain.partial ? <PartialBadge /> : null}
      </div>
      <p className="subtitle">
        <span className="mono">{chain.repo_path}</span> → base{" "}
        <span className="mono">{chain.base}</span> · updated{" "}
        {timeAgo(chain.updated_at)}
      </p>

      {chain.last_scan_error ? (
        <div className="banner banner-error">
          <strong>scan failed</strong>
          <span className="banner-body">{chain.last_scan_error}</span>
        </div>
      ) : null}

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
          {live.length === 0 ? (
            <tr>
              <td colSpan={5}>
                <div className="empty-state" style={{ border: "none" }}>
                  Chain is empty — the branch has no commits over{" "}
                  <code>{chain.base}</code>.
                </div>
              </td>
            </tr>
          ) : (
            live.map((change) => <ChangeRow key={change.id} change={change} />)
          )}
        </tbody>
      </table>

      {orphaned.length > 0 ? (
        <details className="orphaned-block">
          <summary>
            {orphaned.length} orphaned change{orphaned.length > 1 ? "s" : ""} —
            commits left the branch; comments preserved
          </summary>
          <table className="list changes-table">
            <tbody>
              {orphaned.map((change) => (
                <ChangeRow key={change.id} change={change} />
              ))}
            </tbody>
          </table>
        </details>
      ) : null}
    </main>
  );
}
