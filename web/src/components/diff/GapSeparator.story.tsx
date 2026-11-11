import GapSeparator from "./GapSeparator";

const reveal = () => undefined;

/** Five gaps:
 *
 * - one between two hunks, with both stepped buttons and the header of
 *   the hunk below
 * - one over the first hunk, with the up button only
 * - one under the last hunk, with the down button only
 * - one that a single step covers
 * - one that the page cannot reveal.
 */
export function Gaps() {
  return (
    <div className="flex w-[760px] flex-col gap-3">
      <GapSeparator more={25} unit="line" step={10} down up reveal={reveal}>
        <span className="hunk-header">
          @@ -18,6 +18,17 @@ impl TokenRotator
        </span>
      </GapSeparator>
      <GapSeparator
        more={17}
        unit="line"
        step={10}
        down={false}
        up
        reveal={reveal}
      >
        <span className="hunk-header">@@ -18,6 +18,17 @@ use std::time</span>
      </GapSeparator>
      <GapSeparator
        more={40}
        unit="line"
        step={10}
        down
        up={false}
        reveal={reveal}
      />
      <GapSeparator
        more={7}
        unit="line"
        step={10}
        down
        up={false}
        reveal={reveal}
      />
      <GapSeparator more={3} unit="block" step={1} down up />
    </div>
  );
}
