import { useState } from "react";
import { Link } from "react-router-dom";
import type { Chain } from "../api/types";
import { StatusChip, StatusDot } from "./badges";

/**
 * Inline chain context at the right end of the review header's meta line:
 * one status dot per change in chain order (current one ringed, siblings
 * click through — the dashboard's dot pattern) and an `N/M` expand toggle.
 * Expanding renders the chain list (position, subject, unresolved count,
 * status chip per change) in normal flow, pushing the content below down.
 */
export default function ChainStrip({
  chain,
  currentId,
}: {
  chain: Chain | undefined;
  currentId: number;
}) {
  const [open, setOpen] = useState(false);

  // Close the panel when navigation lands on another change (n/p included).
  // Adjust during render, not in an effect, so the panel never paints open
  // for the new change.
  const [seenId, setSeenId] = useState(currentId);
  if (seenId !== currentId) {
    setSeenId(currentId);
    setOpen(false);
  }

  if (!chain) return null;

  const current = chain.changes.find((c) => c.id === currentId);
  const posLabel = `${
    current && current.position !== null ? current.position + 1 : "—"
  }/${chain.changes.length}`;

  return (
    <>
      <span className="chain-strip">
        <span className="dots">
          {chain.changes.map((c) => {
            // The dashboard's dot tooltip format.
            const title = `${(c.position ?? 0) + 1}. ${c.subject} — ${c.status}`;
            return c.id === currentId ? (
              <span className="current" key={c.id}>
                <StatusDot status={c.status} title={`${title} (this change)`} />
              </span>
            ) : (
              <Link key={c.id} to={`/changes/${c.id}`}>
                <StatusDot status={c.status} title={title} />
              </Link>
            );
          })}
        </span>
        <button
          className="chain-strip-toggle mono"
          aria-expanded={open}
          title={open ? "Collapse the chain list" : "Expand the chain list"}
          onClick={() => {
            setOpen((v) => !v);
          }}
        >
          {posLabel} {open ? "▴" : "▾"}
        </button>
      </span>
      {open ? (
        <div className="chain-strip-panel">
          {chain.changes.map((c) => (
            <Link
              key={c.id}
              className={`chain-panel-row ${c.id === currentId ? "current" : ""}`}
              to={`/changes/${c.id}`}
              aria-current={c.id === currentId ? "page" : undefined}
            >
              <span className="pos mono dim">
                {c.position !== null ? c.position + 1 : "—"}
              </span>
              <span className="subj">{c.subject}</span>
              {c.counts.unresolved > 0 ? (
                <span className="unresolved-count" title="unresolved threads">
                  {c.counts.unresolved} open
                </span>
              ) : null}
              <StatusChip status={c.status} />
            </Link>
          ))}
        </div>
      ) : null}
    </>
  );
}
