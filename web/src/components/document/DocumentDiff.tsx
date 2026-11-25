import { useMemo } from "react";
import type { Line, Side } from "../../api/types";
import { type Block, renderDoc } from "../../lib/document";
import { inlineRow, type RowBlock, wholeRow } from "../../lib/inlineDiff";
import { referencesByLine } from "../../lib/reference";
import { sideText } from "../../lib/rendered";
import { useDocRows } from "../diff/useDocRows";
import { DocumentBlock } from "./DocumentBlocks";

const idDistinctAcrossSides = (side: Side, block: Block) =>
  side === "old" ? -block.lines[0] : block.lines[0];

/** The diff of two versions of a document, from all of its `lines`. The
 * changed blocks show, and a separator shows in place of the unchanged
 * blocks far from a change. The blocks have no controls.
 *
 * A changed block of text shows its changes inside the text: an
 * insertion is marked, and a marker stands in place of a deletion, with
 * the removed text on hover. A block that the diff adds or removes whole
 * shows with its mark, as do a reference, a code block, and a block that
 * the diff replaces most of. A removed block then shows above the block
 * that replaces it. */
export default function DocumentDiff({ lines }: { lines: readonly Line[] }) {
  const doc = useMemo(() => renderDoc(lines), [lines]);
  const sides = useMemo(() => {
    const side = (side: Side) => {
      const text = sideText(lines, side);
      return { lines: text.split("\n"), references: referencesByLine(text) };
    };
    return { old: side("old"), new: side("new") };
  }, [lines]);
  const rows = useMemo(() => {
    const reference = (side: Side) => (block: Block) =>
      sides[side].references.has(block.lines[0] - 1);
    return new Map(
      doc.rows.flatMap((row): [typeof row, RowBlock[]][] => {
        if ("same" in row) return [];
        const { removed, added } = row;
        const whole =
          removed.some(reference("old")) || added.some(reference("new"));
        return [
          [
            row,
            whole
              ? wholeRow(removed, added)
              : inlineRow(removed, added, sides.old.lines, sides.new.lines),
          ],
        ];
      }),
    );
  }, [doc, sides]);
  return useDocRows(doc)(
    () => false,
    (row) =>
      (
        rows.get(row) ??
        ("same" in row ? [{ block: row.same, side: row.side, change: "" }] : [])
      ).map(({ block, side, change }) => {
        const id = idDistinctAcrossSides(side, block);
        return (
          <DocumentBlock
            key={id}
            block={block}
            references={sides[side].references}
            id={id}
            change={change}
          />
        );
      }),
  );
}
