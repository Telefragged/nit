import type { Run, Version } from "../../api/document.gen";
import { versionLabel } from "../../lib/documentVersions";
import { timeAgo } from "../../lib/time";

const WRITER = { author: "Author", reviewer: "Reviewer" };

/** The versions of a document, the newest first: the writer and the
 * time of each. A press on a version calls `onSelect` with its index
 * in `runs`. `selected` is the version that the page shows, or null.
 * `sent` is the version last sent to the author. */
export default function VersionList({
  runs,
  sent,
  selected,
  onSelect,
}: {
  runs: readonly Run[];
  sent: Version | null;
  selected: number | null;
  onSelect: (index: number) => void;
}) {
  if (runs.length === 0) return null;
  return (
    <section className="change-log version-list">
      <div className="thread-group-title">Versions</div>
      <div className="log-rows">
        {runs
          .map((run, index) => (
            <div className="log-row" key={run.version}>
              <button
                type="button"
                className="log-line version-row"
                aria-current={selected === index}
                title="Show what this version changed"
                onClick={() => {
                  onSelect(index);
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
          ))
          .reverse()}
      </div>
    </section>
  );
}
