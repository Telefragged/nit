import { useState } from "react";
import DiffRangeSelect from "./DiffRangeSelect";

const COMMENTS = [3, 0, 1];
const WRITERS = ["author", "reviewer", "author"];

/** The revisions of a change, with the diff from revision 0 to revision
 * 2. */
export function Revisions() {
  const [range, setRange] = useState({ selected: 2, against: "r0" });
  return (
    <div className="diffbar-mode">
      <DiffRangeSelect
        items={COMMENTS}
        option={(comments, index) => ({
          label: `r${index}`,
          detail: comments > 0 ? `${comments} comments` : undefined,
        })}
        baseValue={(_, index) => `r${index}`}
        first={{ value: "base", label: "Base" }}
        selected={range.selected}
        against={range.against}
        baseTitle="Base = parent commit; rM = interdiff against revision M"
        label="Revision"
        title="Revision under review"
        placeholder="no revisions"
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

/** The versions of a document, with the diff that version 0 made. */
export function Versions() {
  const [range, setRange] = useState({ selected: 0, against: "-" });
  return (
    <div className="diffbar-mode">
      <DiffRangeSelect
        items={WRITERS}
        option={(writer, index) => ({ label: `v${index}`, detail: writer })}
        baseValue={(_, index) => String(index)}
        first={{ value: "-", label: "Empty" }}
        selected={range.selected}
        against={range.against}
        baseTitle="Empty = the document with no text; vM = the diff from version M"
        label="Version"
        title="The version that the diff ends at"
        placeholder="no versions"
        onLeft={(against) => {
          setRange({ ...range, against });
        }}
        onRight={(selected) => {
          setRange({
            selected,
            against: selected > 0 ? String(selected - 1) : "-",
          });
        }}
      />
    </div>
  );
}
