import { useState } from "react";
import { Link } from "react-router-dom";
import type { Chain } from "../api/types";
import { NewerElsewhereBadge, StatusDot } from "./badges";

/**
 * Chain navigation in the review sidebar, above the file list: one row per
 * path member (status dot, position, subject, unresolved count), the current
 * one highlighted and siblings linking through. Sitting on top fixes its
 * position so the rows stay put when you click between changes — the file
 * list below is the part whose length varies per change, so it (not the
 * chain) absorbs the reflow. A disclosure header collapses the list to give
 * the file list below more room; the list scrolls within its own height cap
 * when the chain itself is long (styles.css). The row layout mirrors the
 * file rail's, so the two stacked lists read as one. A member pinned to an
 * older revision than its latest carries a NEWER ELSEWHERE badge inline.
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

  const current = chain.path.find((c) => c.change_id === currentId);
  const posLabel = `${current ? current.position + 1 : "—"}/${
    chain.path.length
  }`;

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
          {chain.path.map((c) => {
            const pos = c.position + 1;
            const title = `${pos}. ${c.subject} — ${c.status}`;
            const inner = (
              <>
                <StatusDot status={c.status} />
                <span className="pos mono dim">{pos}</span>
                <span className="subj">{c.subject}</span>
                {c.newer_elsewhere ? (
                  <NewerElsewhereBadge
                    revision={c.revision}
                    latest={c.latest_revision}
                  />
                ) : null}
                {c.counts.unresolved > 0 ? (
                  <span className="unresolved-count" title="unresolved threads">
                    {c.counts.unresolved} open
                  </span>
                ) : null}
              </>
            );
            return c.change_id === currentId ? (
              <div
                key={c.change_id}
                className="chain-nav-row current"
                aria-current="page"
                title={`${title} (this change)`}
              >
                {inner}
              </div>
            ) : (
              <Link
                key={c.change_id}
                className="chain-nav-row"
                to={`/changes/${c.change_id}`}
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
