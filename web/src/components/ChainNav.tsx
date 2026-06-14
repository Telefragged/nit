import { useState } from "react";
import { Link } from "react-router-dom";
import type { Chain } from "../api/types";
import { StatusDot } from "./badges";

/**
 * Chain navigation in the review sidebar, below the file list: one row per
 * change (status dot, position, subject, unresolved count), the current one
 * highlighted and siblings linking through. A disclosure header collapses
 * the list to reclaim sidebar height for a long file list; the list scrolls
 * within its own height cap when the chain itself is long (styles.css). The
 * row layout mirrors the file rail's, so the two stacked lists read as one.
 */
export default function ChainNav({
  chain,
  currentId,
}: {
  chain: Chain | undefined;
  currentId: number;
}) {
  const [open, setOpen] = useState(true);
  if (!chain) return null;

  const current = chain.changes.find((c) => c.id === currentId);
  const posLabel = `${
    current && current.position !== null ? current.position + 1 : "—"
  }/${chain.changes.length}`;

  return (
    <section className="chain-nav">
      <button
        className="chain-nav-title"
        aria-expanded={open}
        title={open ? "Collapse the chain list" : "Expand the chain list"}
        onClick={() => {
          setOpen((v) => !v);
        }}
      >
        <span className="fchevron">{open ? "▾" : "▸"}</span>
        <span>chain</span>
        <span className="chain-nav-pos mono">{posLabel}</span>
      </button>
      {open ? (
        <div className="chain-nav-list">
          {chain.changes.map((c) => {
            const pos = c.position !== null ? c.position + 1 : "—";
            const title = `${pos}. ${c.subject} — ${c.status}`;
            const inner = (
              <>
                <StatusDot status={c.status} />
                <span className="pos mono dim">{pos}</span>
                <span className="subj">{c.subject}</span>
                {c.counts.unresolved > 0 ? (
                  <span className="unresolved-count" title="unresolved threads">
                    {c.counts.unresolved} open
                  </span>
                ) : null}
              </>
            );
            return c.id === currentId ? (
              <div
                key={c.id}
                className="chain-nav-row current"
                aria-current="page"
                title={`${title} (this change)`}
              >
                {inner}
              </div>
            ) : (
              <Link
                key={c.id}
                className="chain-nav-row"
                to={`/changes/${c.id}`}
                title={title}
              >
                {inner}
              </Link>
            );
          })}
        </div>
      ) : null}
    </section>
  );
}
