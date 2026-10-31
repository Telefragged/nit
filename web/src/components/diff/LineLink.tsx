import { memo } from "react";
import type { Side } from "../../api/types";
import { pressLink } from "../../lib/diffLink";
import { plainClick } from "../../lib/shortcutKey";
import type { Press } from "../../lib/useLineDrag";
import { useReview } from "../../pages/reviewContext";

/** A line number in a gutter, as a link to lines `start` through `end`:
 * one line, or the lines of a rendered block, which it labels by its first
 * line. A press links them, and a drag from it links a span
 * (lib/useLineDrag). The lines it names are its `data-side`, `data-start`
 * and `data-end`, which a drag reads (lib/selection `rowLines`). */
function LineLink({
  file,
  side,
  start,
  end,
  press,
  className = "",
}: {
  file: string;
  side: Side;
  start: number;
  end: number;
  press: Press;
  className?: string;
}) {
  const ctx = useReview();
  return (
    <a
      className={`line-link${className}`}
      href={ctx.linkHref({ file, lines: { side, start, end } })}
      title={start === end ? undefined : `Lines ${start}–${end}`}
      data-side={side}
      data-start={start}
      data-end={end}
      onMouseDown={(e) => {
        if (!plainClick(e)) return;
        // Keeps the browser from dragging the link or selecting text.
        e.preventDefault();
        press(side, [start, end], e.shiftKey);
      }}
      onClick={(e) => {
        if (!plainClick(e)) return;
        e.preventDefault();
        // A click from the keyboard has no mousedown before it, so it
        // links the lines here.
        if (e.detail === 0) {
          ctx.selectLink(
            pressLink(ctx.link, file, side, [start, end], e.shiftKey).span,
          );
        }
      }}
    >
      {start}
    </a>
  );
}

// A section re-renders on every hover. memo keeps its line numbers from
// rendering again.
export default memo(LineLink);
