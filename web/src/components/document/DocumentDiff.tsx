import { useMemo } from "react";
import type { Line, Side } from "../../api/types";
import { type Block, renderDoc } from "../../lib/document";
import { inlineRow, type RowBlock, wholeRow } from "../../lib/inlineDiff";
import { referencesByLine } from "../../lib/reference";
import { sideText } from "../../lib/rendered";
import { DocumentBlock } from "./DocumentBlocks";

const idDistinctAcrossSides = (side: Side, block: Block) =>
  side === "old" ? -block.lines[0] : block.lines[0];

/** The diff of two versions of a document, from all of its `lines`:
 * every block of the new text, and the blocks that the diff removes. The
 * blocks have no controls.
 *
 * A changed block of text shows its changes inside the text: an
 * insertion is marked, and a deletion shows at its place, marked as
 * removed. A block that the diff adds whole has a
 * green line on its left, and a block that the diff removes whole has a
 * red line. A reference, a code block, and a block that the diff
 * replaces most of show in the same way: the old block above the new
 * block.
 *
 * With `only`, the lines have no change, and the component renders that
 * side of them. */
export default function DocumentDiff({
  lines,
  only,
}: {
  lines: readonly Line[];
  only?: Side;
}) {
  const doc = useMemo(() => renderDoc(lines, only), [lines, only]);
  const sides = useMemo(() => {
    const side = (side: Side) => {
      const text = sideText(lines, side);
      return { lines: text.split("\n"), references: referencesByLine(text) };
    };
    const shown = side(only ?? "new");
    return { old: only ? shown : side("old"), new: shown };
  }, [lines, only]);
  const blocks = useMemo(() => {
    const reference = (side: Side) => (block: Block) =>
      sides[side].references.has(block.lines[0] - 1);
    return doc.rows.flatMap((row): RowBlock[] => {
      if ("same" in row)
        return [{ block: row.same, side: row.side, change: "" }];
      const { removed, added } = row;
      const whole =
        removed.some(reference("old")) || added.some(reference("new"));
      return whole
        ? wholeRow(removed, added)
        : inlineRow(removed, added, sides.old.lines, sides.new.lines);
    });
  }, [doc, sides]);
  return blocks.map(({ block, side, change }) => {
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
  });
}
