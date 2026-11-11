import { ArrowDown, ArrowUp } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "../ui/button";

type End = "down" | "up";

/** A separator in place of a gap of `more` hidden lines or blocks.
 *
 * The separator renders only when `more` is not 0. It shows the count,
 * then `children`.
 *
 * When the caller passes `reveal`, buttons call it. One button shows the
 * whole gap: from the top edge when `down` is true, and from the bottom
 * edge when it is false. When `up` is true, a button with an up arrow
 * shows the last `step` rows, over the rows below the gap. When `down` is
 * true, a button with a down arrow shows the first `step` rows, under the
 * rows above the gap. A gap that one step covers has no stepped buttons. */
export default function GapSeparator({
  more,
  unit,
  step,
  down,
  up,
  reveal,
  busy = () => false,
  children,
}: {
  more: number;
  unit: "line" | "block";
  step: number;
  down: boolean;
  up: boolean;
  reveal?: (end: End, count: number) => void;
  busy?: (end: End) => boolean;
  children?: ReactNode;
}) {
  if (more === 0) return null;
  const noun = (n: number) => (n === 1 ? unit : `${unit}s`);
  const stepped = more > step;
  const button = (kind: "all" | End, count: number, title: string) => {
    const end = kind !== "all" ? kind : down ? "down" : "up";
    const Arrow = kind === "down" ? ArrowDown : ArrowUp;
    return (
      <Button
        variant="ghost"
        size="sm"
        className={`expand-${kind} h-5 px-1.5`}
        onClick={() => {
          reveal?.(end, kind === "all" ? Infinity : count);
        }}
        disabled={busy(end)}
        title={title}
      >
        {kind === "all" ? (
          "Show all"
        ) : (
          <>
            <Arrow size={12} aria-hidden />
            {count}
          </>
        )}
      </Button>
    );
  };
  return (
    <div className="hunk-row">
      <span className="hunk-label">
        {more} {noun(more)} hidden
        {children}
      </span>
      {reveal ? (
        <span className="hunk-expanders">
          {stepped && up
            ? button("up", step, `Show ${step} more ${noun(step)} above`)
            : null}
          {button("all", more, `Show all ${more} ${noun(more)}`)}
          {stepped && down
            ? button("down", step, `Show ${step} more ${noun(step)} below`)
            : null}
        </span>
      ) : null}
    </div>
  );
}
