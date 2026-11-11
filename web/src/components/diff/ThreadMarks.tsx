import { markCounts, type UiThread } from "../../lib/comments";

/** `threads` counted by mark: a pill with each count, then the word for
 * the mark. */
export default function ThreadMarks({
  threads,
}: {
  threads: readonly UiThread[];
}) {
  return markCounts(threads).map(({ mark, count, word }) => (
    <span key={mark} className="count">
      <span className={`pill mark-${mark}`}>{count}</span> {word}
    </span>
  ));
}
