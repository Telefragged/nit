import { useMemo } from "react";
import type { Line } from "../../api/types";
import { renderDoc } from "../../lib/document";
import { referencesByLine } from "../../lib/reference";
import { sideText } from "../../lib/rendered";
import { useDocRows } from "../diff/useDocRows";
import { DocumentBlock } from "./DocumentBlocks";

/** The diff of two versions of a document, from all of its `lines`, as
 * the review page renders the diff of a Markdown file. The changed
 * blocks show, each with its mark, and a removed block shows above the
 * block that replaces it. A separator shows in place of the unchanged
 * blocks far from a change. The blocks have no controls. The id of a
 * block of the old text is negative, so that it differs from the id of
 * every block of the new text. */
export default function DocumentDiff({ lines }: { lines: readonly Line[] }) {
  const doc = useMemo(() => renderDoc(lines), [lines]);
  const references = useMemo(
    () => ({
      old: referencesByLine(sideText(lines, "old")),
      new: referencesByLine(sideText(lines, "new")),
    }),
    [lines],
  );
  return useDocRows(doc)(
    () => false,
    (row) =>
      [
        ...("same" in row
          ? [{ block: row.same, side: row.side, change: "" }]
          : [
              ...row.removed.map((block) => ({
                block,
                side: "old" as const,
                change: " doc-removed",
              })),
              ...row.added.map((block) => ({
                block,
                side: "new" as const,
                change: " doc-added",
              })),
            ]),
      ].map(({ block, side, change }) => {
        const id = side === "old" ? -block.lines[0] : block.lines[0];
        return (
          <DocumentBlock
            key={id}
            block={block}
            references={references[side]}
            id={id}
            change={change}
          />
        );
      }),
  );
}
