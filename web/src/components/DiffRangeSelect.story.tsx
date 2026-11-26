import { useState } from "react";
import DiffRangeSelect from "./DiffRangeSelect";

/** The revisions of a change, with the diff from revision 0 to revision
 * 2. */
export function Revisions() {
  const [range, setRange] = useState({ selected: 2, against: "r0" });
  return (
    <div className="diffbar-mode">
      <DiffRangeSelect
        details={["3 comments", undefined, "1 comment"]}
        selected={range.selected}
        against={range.against}
        onLeft={(against) => {
          setRange({ ...range, against });
        }}
        onRight={(selected) => {
          setRange({ selected, against: "base" });
        }}
      />
    </div>
  );
}
