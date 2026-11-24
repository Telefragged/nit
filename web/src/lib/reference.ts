import type {
  LineRange,
  PlacedReference,
  Reference,
} from "../api/document.gen";
import type { ChangeVersion, Hunk, Line, Side } from "../api/types";
import type { ReviewFile } from "./attachments";
import { commentPlacement, shownAt, type UiThread } from "./comments";
import { type DiffLink, linkParams } from "./diffLink";
import { patchParams } from "./useUrlParams";
import { isBase } from "./version";
import { reference_address, references } from "./wasm";

/** The reference of each paragraph of the Markdown `text` that is one, by
 * the first line of the paragraph, counted from 0. `references` of
 * `nit-document` defines which paragraph is a reference. */
export const referencesByLine = (text: string): Map<number, Reference> =>
  new Map(
    (references(text) as PlacedReference[]).map((placed) => [
      placed.line,
      placed.reference,
    ]),
  );

/** The Markdown of a reference to an upload, for a line of its own: an
 * image for `image`, or else a link. */
export function uploadMarkdown(hash: string, name: string, image: boolean) {
  const label = name.replace(/[\\[\]]/g, "\\$&");
  const address = reference_address({ type: "upload", hash, name });
  return `${image ? "!" : ""}[${label}](${address})`;
}

/** The address of the review page of `change` at the diff range from
 * `against` to `selected`, at `link` when it is not null. */
export function changeHref(
  change: number,
  selected: number,
  against: ChangeVersion,
  link: DiffLink | null,
): string {
  const params = patchParams(new URLSearchParams(), {
    revision: String(selected),
    against: isBase(against) ? null : against,
    ...(link && linkParams(link)),
  });
  return `/changes/${change}?${params.toString()}`;
}

function hunkStart(
  whole: readonly Line[],
  first: number,
  lines: readonly Line[],
  side: Side,
): number {
  const own = lines.find((line) => line[side] !== undefined)?.[side];
  if (own !== undefined) return own;
  for (let i = first - 1; i >= 0; i--) {
    const before = whole[i]?.[side];
    if (before !== undefined) return before;
  }
  return 0;
}

/** `file` with one hunk: the lines of `range`, and each line of the other
 * side between them. `whole` is the whole file as diff lines. Null when
 * the file lacks the first or the last line of the range. */
export function rangeFile(
  file: ReviewFile,
  whole: readonly Line[],
  range: LineRange,
): ReviewFile | null {
  const first = whole.findIndex((line) => line[range.side] === range.start);
  const last = whole.findIndex((line) => line[range.side] === range.end);
  if (first === -1 || last === -1) return null;
  const lines = whole.slice(first, last + 1);
  const count = (side: Side) =>
    lines.filter((line) => line[side] !== undefined).length;
  const hunk: Hunk = {
    old_start: hunkStart(whole, first, lines, "old"),
    old_lines: count("old"),
    new_start: hunkStart(whole, first, lines, "new"),
    new_lines: count("new"),
    header: "",
    lines,
  };
  return { ...file, hunks: [hunk] };
}

/** The threads that the diff range from `against` to `selected` shows on
 * one of `lines`. */
export function threadsOnLines(
  threads: readonly UiThread[],
  lines: readonly Line[],
  selected: number,
  against: ChangeVersion,
): UiThread[] {
  return threads.filter((thread) => {
    const place = commentPlacement(shownAt(thread), selected, against);
    return (
      place !== null && lines.some((line) => line[place.side] === place.line)
    );
  });
}
