import GapSeparator from "./GapSeparator";

const reveal = () => undefined;

/** Three gaps: one between two hunks, with both stepped buttons and the
 * header of the hunk below; one that a single step covers; and one that
 * the page cannot reveal. */
export function Gaps() {
  return (
    <div className="flex w-[760px] flex-col gap-3">
      <GapSeparator more={25} unit="line" step={10} down up reveal={reveal}>
        <span className="hunk-header">
          @@ -18,6 +18,17 @@ impl TokenRotator
        </span>
      </GapSeparator>
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
