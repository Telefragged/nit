import { MessageSquare } from "lucide-react";
import { markWord, type ThreadMark } from "../lib/comments";

/** A count of threads with one mark: the comment icon in the form of the
 * mark, then the count. With `word`, the word for the mark follows the
 * count. Without it, the word is in the title. */
export default function ThreadCount({
  mark,
  count,
  word = false,
}: {
  mark: ThreadMark;
  count: number;
  word?: boolean;
}) {
  const label = markWord(mark, count);
  return (
    <span className="count" title={word ? undefined : `${count} ${label}`}>
      <MessageSquare
        className={`thread-icon mark-${mark}`}
        size={14}
        aria-hidden
      />
      {count}
      {word ? ` ${label}` : null}
    </span>
  );
}
