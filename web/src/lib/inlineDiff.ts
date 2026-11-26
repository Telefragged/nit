import type { CommentRange, Side } from "../api/types";
import { type Block, changedElement, type Deletion } from "./document";
import type { SourcePlace } from "./sourceMap";
import { text_replacements } from "./wasm";

/** One block of a changed row of a diff, as the row shows it. `change`
 * is the class suffix of a block that the diff adds or removes whole. */
export interface RowBlock {
  block: Block;
  side: Side;
  change: string;
}

type CharRange = [start: number, endExclusive: number];

interface Replacement {
  old: CharRange;
  new: CharRange;
}

const MAX_REPLACED_SHARE_FOR_INLINE = 0.5;

const wholeBlock = (block: Block, side: Side): RowBlock => ({
  block,
  side,
  change: side === "old" ? " doc-removed" : " doc-added",
});

/** The blocks of a changed row, each whole: the old blocks, then the new
 * blocks. */
export const wholeRow = (
  removed: readonly Block[],
  added: readonly Block[],
): RowBlock[] => [
  ...removed.map((block) => wholeBlock(block, "old")),
  ...added.map((block) => wholeBlock(block, "new")),
];

const covers = ([from, to]: CharRange, [start, end]: CharRange) =>
  from <= start && end <= to;

const length = (spans: readonly CharRange[]) =>
  spans.reduce((sum, [start, end]) => sum + end - start, 0);

function textOfRowSide(blocks: readonly Block[], lines: readonly string[]) {
  const first = blocks[0]?.lines[0] ?? 1;
  const own = lines.slice(first - 1, blocks.at(-1)?.lines[1] ?? 0);
  const starts: number[] = [];
  let at = 0;
  for (const line of own) {
    starts.push(at);
    at += line.length + 1;
  }
  const offset = (line: number) => starts[line - first] ?? at;
  return {
    text: own.join("\n"),
    blocks: blocks.map((block) => {
      const [start, end] = block.lines;
      const span: CharRange = [
        offset(start),
        offset(end) + (own[end - first]?.length ?? 0),
      ];
      return { block, span };
    }),
    place: (char: number): SourcePlace => {
      let line = 0;
      while ((starts[line + 1] ?? Infinity) <= char) line++;
      return { line: first + line, char: char - (starts[line] ?? 0) };
    },
  };
}

type RowSide = ReturnType<typeof textOfRowSide>;
type SpannedBlock = RowSide["blocks"][number];

function blocksRemovedWhole(
  blocks: readonly SpannedBlock[],
  replacements: readonly Replacement[],
) {
  return blocks.flatMap(({ block, span }) => {
    const by = replacements.find((r) => covers(r.old, span));
    return by ? [{ block, span, placeInNewText: by.new[0] }] : [];
  });
}

function replacesMost(
  replacements: readonly Replacement[],
  wholeSpans: readonly CharRange[],
  lengthOfBothTexts: number,
) {
  const inWholeBlocks = length(wholeSpans);
  const replaced = length(replacements.flatMap((r) => [r.old, r.new]));
  return (
    replaced - inWholeBlocks >
    MAX_REPLACED_SHARE_FOR_INLINE * (lengthOfBothTexts - inWholeBlocks)
  );
}

function deletionsByBlock(
  replacements: readonly Replacement[],
  removedWhole: readonly { span: CharRange }[],
  changedBlocks: readonly SpannedBlock[],
  oldText: string,
  now: RowSide,
) {
  const deleted = new Map<Block, Deletion[]>();
  for (const r of replacements) {
    let text = "";
    let from = r.old[0];
    for (const { span } of removedWhole) {
      if (!covers(r.old, span)) continue;
      text += oldText.slice(from, span[0]);
      from = span[1];
    }
    text += oldText.slice(from, r.old[1]);
    const firstBlockEndingAtOrAfter = changedBlocks.find(
      (item) => item.span[1] >= r.new[0],
    );
    const owner = firstBlockEndingAtOrAfter ?? changedBlocks.at(-1);
    if (text.trim() === "" || !owner) continue;
    const list = deleted.get(owner.block) ?? [];
    list.push({ at: now.place(r.new[0]), text });
    deleted.set(owner.block, list);
  }
  return deleted;
}

/** The blocks that a changed row shows, with the changes inside the
 * text of the new blocks: an insertion and a deletion are marked. A block that the diff adds or removes whole shows
 * whole.
 *
 * The row shows its old and new blocks whole (`wholeRow`) when one side
 * has no block, when a block is a code block, or when the diff replaces
 * most of the text. `oldLines` and `newLines` are all the lines of the
 * two texts. */
export function inlineRow(
  removed: readonly Block[],
  added: readonly Block[],
  oldLines: readonly string[],
  newLines: readonly string[],
): RowBlock[] {
  const whole = wholeRow(removed, added);
  if (removed.length === 0 || added.length === 0) return whole;
  if ([...removed, ...added].some((block) => block.node.tagName === "pre"))
    return whole;
  const old = textOfRowSide(removed, oldLines);
  const now = textOfRowSide(added, newLines);
  const replacements = text_replacements(old.text, now.text) as Replacement[];

  const gone = blocksRemovedWhole(old.blocks, replacements);
  const news = now.blocks.map((item) => ({
    ...item,
    whole: replacements.some((r) => covers(r.new, item.span)),
  }));
  const within = news.filter((item) => !item.whole);
  const wholeSpans = [...gone, ...news.filter((item) => item.whole)].map(
    (item) => item.span,
  );
  if (
    replacements.length === 0 ||
    within.length === 0 ||
    replacesMost(replacements, wholeSpans, old.text.length + now.text.length)
  )
    return whole;
  const deleted = deletionsByBlock(replacements, gone, within, old.text, now);

  const rows: RowBlock[] = [];
  let pending = gone;
  for (const { block, span, whole } of news) {
    const due = pending.filter((item) => item.placeInNewText <= span[0]);
    pending = pending.slice(due.length);
    rows.push(...due.map((item) => wholeBlock(item.block, "old")));
    if (whole) {
      rows.push(wholeBlock(block, "new"));
      continue;
    }
    const inserted = replacements.flatMap((r): CommentRange[] => {
      const from = Math.max(r.new[0], span[0]);
      const to = Math.min(r.new[1], span[1]);
      if (from >= to) return [];
      const [first, last] = [now.place(from), now.place(to)];
      return [
        {
          start_line: first.line,
          start_char: first.char,
          end_line: last.line,
          end_char: last.char,
        },
      ];
    });
    const node = changedElement(block, inserted, deleted.get(block) ?? []);
    rows.push({ block: { ...block, node }, side: "new", change: "" });
  }
  rows.push(...pending.map((item) => wholeBlock(item.block, "old")));
  return rows;
}
