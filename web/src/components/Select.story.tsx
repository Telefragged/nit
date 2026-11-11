import { useState } from "react";
import Select, { type SelectOption } from "./Select";

const TAGS: SelectOption[] = [
  { value: "branch", label: "branch", detail: "track/auth-rotation" },
  { value: "session-id", label: "session-id", detail: "auth-rotation" },
  { value: "worktree", label: "worktree", detail: ".worktrees/auth-rotation" },
];

const REVISIONS: SelectOption[] = [
  { value: "0", label: "r0", detail: "6 comments" },
  { value: "1", label: "r1", detail: "3 comments" },
  { value: "2", label: "r2", disabled: true },
];

/** A picker as wide as the tag selector of the review page. The space
 * below it shows the open list. */
function Picker({
  label,
  options,
  initial,
}: {
  label: string;
  options: SelectOption[];
  initial: string;
}) {
  const [value, setValue] = useState(initial);
  return (
    <div style={{ display: "flex", width: 420, minHeight: 110 }}>
      <Select
        label={label}
        placeholder="no tags"
        value={value}
        onChange={setValue}
        options={options}
      />
    </div>
  );
}

/** The tags of a change: each key with its value. */
export function Tags() {
  return <Picker label="Tag" options={TAGS} initial="session-id" />;
}

/** The revisions of a change, with a revision that the page cannot show. */
export function Revisions() {
  return <Picker label="Revision" options={REVISIONS} initial="1" />;
}

/** A change without a tag. */
export function Empty() {
  return <Picker label="Tag" options={[]} initial="" />;
}
