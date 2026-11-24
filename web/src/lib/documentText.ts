import type { Lines } from "./diffLink";

/** The source of the block on `lines` of a text. `all` is every line of
 * the text. Lines count from 1. */
export const blockSource = (all: readonly string[], [start, end]: Lines) =>
  all.slice(start - 1, end).join("\n");

/** `text` with `source` in place of the block on `lines`. The indent of
 * the first line of the block remains, because a text editor saves its
 * text without outer whitespace. */
export function withBlock(text: string, lines: Lines, source: string) {
  const [start, end] = lines;
  const all = text.split("\n");
  const indent = /^\s*/.exec(all[start - 1] ?? "")?.[0] ?? "";
  all.splice(start - 1, end - start + 1, indent + source);
  return all.join("\n");
}

/** `text` without the block on `lines`. The blank line below the block
 * goes with it, unless that line separates the blocks above and below. */
export function withoutBlock(text: string, [start, end]: Lines) {
  const all = text.split("\n");
  all.splice(start - 1, end - start + 1);
  const blank = (line: string | undefined) => line?.trim() === "";
  const above = all[start - 2];
  if (blank(all[start - 1]) && (above === undefined || blank(above)))
    all.splice(start - 1, 1);
  return all.join("\n");
}
