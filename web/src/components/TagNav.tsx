import { memo, useState } from "react";
import { Link } from "react-router-dom";
import type { ChangeDetail, Tags } from "../api/types";
import { revisionActivity } from "../lib/comments";
import { StatusDot } from "./badges";

/**
 * The review sidebar's list of the changes that share a tag with the
 * current one, above the file list: one row per change (status dot,
 * position, subject, unresolved count), the current one highlighted and
 * the others linking through. The header's selector picks which of the
 * current change's tag keys the list follows; a change with no tags
 * disables it. Sitting on top fixes the list's position, so the rows stay
 * put when you click between changes and the file list below absorbs the
 * reflow. A disclosure collapses the list; the list scrolls within its
 * own height cap (styles/review.css).
 */
export default function TagNav({
  tags,
  selectedKey,
  onSelectKey,
  members,
  currentId,
}: {
  /** The current change's tags: the keys the selector offers. */
  tags: Tags;
  /** The key the list follows; null only when `tags` is empty. */
  selectedKey: string | null;
  onSelectKey: (key: string) => void;
  /** Every change carrying the selected key's value, ascending by number. */
  members: ChangeDetail[];
  currentId: number;
}) {
  const [open, setOpen] = useState(true);
  const keys = Object.keys(tags);
  const position = members.findIndex((m) => m.id === currentId);
  const posLabel = `${position < 0 ? "—" : position + 1}/${members.length}`;

  return (
    <section className="tag-nav">
      <div className="tag-nav-title">
        <button
          className="tag-nav-toggle"
          aria-expanded={open}
          title={open ? "Collapse the change list" : "Expand the change list"}
          onClick={() => {
            setOpen((v) => !v);
          }}
        >
          <span className="fchevron">{open ? "▾" : "▸"}</span>
        </button>
        <select
          className="revision-select"
          aria-label="Tag"
          title="List the changes that share this tag's value"
          disabled={selectedKey === null}
          value={selectedKey ?? ""}
          onChange={(e) => {
            onSelectKey(e.target.value);
          }}
        >
          {selectedKey === null ? <option value="">no tags</option> : null}
          {keys.map((key) => (
            <option key={key} value={key}>
              {key}
            </option>
          ))}
        </select>
        <span className="tag-nav-pos mono">{posLabel}</span>
      </div>
      {open ? (
        <div className="tag-nav-list">
          {members.map((m, i) => (
            <Row key={m.id} member={m} index={i} current={m.id === currentId} />
          ))}
        </div>
      ) : null}
    </section>
  );
}

/** One change's row. Memoized, so a projection arriving for one member
 * re-renders that row alone. */
const Row = memo(function Row({
  member,
  index,
  current,
}: {
  member: ChangeDetail;
  index: number;
  current: boolean;
}) {
  const latest = member.revisions.at(-1);
  const subject = latest?.subject ?? "";
  const status = latest?.status ?? "pending";
  const title = `${index + 1}. ${subject} — ${status}`;
  const unresolved = latest
    ? revisionActivity(member.threads, member.drafts, latest.number).unresolved
    : 0;
  const inner = (
    <>
      <StatusDot status={status} />
      <span className="pos mono dim">{index + 1}</span>
      <span className="subj">{subject}</span>
      {unresolved > 0 ? (
        <span className="unresolved-count" title="unresolved threads">
          {unresolved} open
        </span>
      ) : null}
    </>
  );
  return current ? (
    <div
      className="tag-nav-row current"
      aria-current="page"
      title={`${title} (this change)`}
    >
      {inner}
    </div>
  ) : (
    <Link className="tag-nav-row" to={`/changes/${member.id}`} title={title}>
      {inner}
    </Link>
  );
});
