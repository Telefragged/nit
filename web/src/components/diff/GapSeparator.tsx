import type { ReactNode } from "react";

type End = "down" | "up";

/** A separator in place of a gap of `more` hidden lines or blocks.
 *
 * The separator renders only when `more` is not 0. When the caller passes
 * `reveal`, buttons over the middle of the separator call it. The button
 * for the whole gap reveals from the top edge when `down` is true, and
 * from the bottom edge when it is false. When `down` is true, a button
 * shows the first `step` rows of the gap, under the rows above it. When
 * `up` is true, a button shows the last `step` rows, over the rows below
 * it. A gap that one step covers has no stepped buttons. `children` render
 * after the count. */
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
  const button = (kind: "all" | End, count: number, title: string) => {
    const end = kind !== "all" ? kind : down ? "down" : "up";
    return (
      <button
        type="button"
        className={`hunk-expand expand-${kind}`}
        onClick={() => {
          reveal?.(end, kind === "all" ? Infinity : count);
        }}
        disabled={busy(end)}
        title={title}
      >
        +{count}
      </button>
    );
  };
  return (
    <div className="hunk-row">
      {reveal ? (
        <div className="hunk-expanders">
          {button("all", more, `Show all ${more} ${noun(more)}`)}
          {more > step ? (
            <div className="hunk-expand-steps">
              {down
                ? button("down", step, `Show ${step} more ${noun(step)} below`)
                : null}
              {up
                ? button("up", step, `Show ${step} more ${noun(step)} above`)
                : null}
            </div>
          ) : null}
        </div>
      ) : null}
      <span className="hunk-skip">
        ⋯ {more} {noun(more)}
      </span>
      {children}
    </div>
  );
}
