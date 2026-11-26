import type { Run, Version } from "../../api/document.gen";
import { type VersionRange, versionLabel } from "../../lib/documentVersions";
import { timeAgo } from "../../lib/time";

const WRITER = { author: "Author", reviewer: "Reviewer" };

/** The versions of a document, the oldest first: the writer and the
 * time of each. A press on a version calls `onSelect` with its index
 * in `runs`, and with `extend` when the Shift key is down. `selected` is
 * the range of versions that the page shows, or null. `sent` is
 * the version last sent to the author. */
export default function VersionList({
  runs,
  sent,
  selected,
  onSelect,
}: {
  runs: readonly Run[];
  sent: Version | null;
  selected: VersionRange | null;
  onSelect: (index: number, extend: boolean) => void;
}) {
  if (runs.length === 0) return null;
  return (
    <div className="log-rows">
      {runs.map((run, index) => (
        <div className="log-row" key={run.version}>
          <button
            type="button"
            className="log-line version-row"
            aria-current={
              selected !== null &&
              selected.from <= index &&
              index <= selected.to
            }
            title="Show this version. With Shift: show the changes between the version on screen and this version"
            onClick={(e) => {
              onSelect(index, e.shiftKey);
            }}
          >
            <span className="log-kind mono">{versionLabel(index)}</span>
            <span className="log-summary">
              <span className={`byline byline-${run.writer}`}>
                {WRITER[run.writer]}
              </span>
              {run.version === sent ? (
                <span className="dim">, sent to the author</span>
              ) : null}
            </span>
            <span className="dim log-time" title={run.at}>
              {timeAgo(run.at)}
            </span>
          </button>
        </div>
      ))}
    </div>
  );
}
