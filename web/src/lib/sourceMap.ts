import type { CommentRange } from "../api/types";
import { rangeSliceOnLine } from "./diffview";

/** A place in the source: a 1-based line and a 0-based character. */
export interface SourcePlace {
  line: number;
  char: number;
}

/** A run of rendered characters that lie one after another on one
 * source line. It starts at rendered index `at`, which comes from
 * `char` of `line`. */
interface SourceRun extends SourcePlace {
  at: number;
}

/** The runs of one rendered text, in order. The first run starts at
 * index 0. */
export type SourceMap = SourceRun[];

const ENTITY = /&(?:#\d{1,7}|#[xX][\da-fA-F]{1,6}|[A-Za-z][A-Za-z\d]{1,31});/y;

/** The map of `value`, the text that Markdown renders from `source`,
 * where `source` starts at `start`.
 *
 * Each rendered character comes from the next source character equal to
 * it. The source characters in between do not render: a backslash
 * escape, the marker of a block quote, the indent of a list item. A
 * character reference, such as `&copy;`, renders as one character. A
 * character not found in the rest of the source maps to the place where
 * the search started. */
export function alignText(
  value: string,
  source: string,
  start: SourcePlace,
): SourceMap {
  const map: SourceMap = [];
  let j = 0;
  let { line, char } = start;
  const advance = (to: number) => {
    for (; j < to; j++) {
      if (source[j] === "\n") {
        line++;
        char = 0;
      } else {
        char++;
      }
    }
  };
  for (let i = 0; i < value.length; i++) {
    const c = value[i];
    let k = j;
    let width = 0;
    for (; k < source.length; k++) {
      if (source[k] === c) {
        width = 1;
        break;
      }
      ENTITY.lastIndex = k;
      const entity = source[k] === "&" && c !== "&" && ENTITY.exec(source);
      if (entity) {
        width = entity[0].length;
        break;
      }
    }
    if (width === 0) k = j;
    advance(k);
    const last = map.at(-1);
    if (last?.line !== line || last.char + (i - last.at) !== char) {
      map.push({ at: i, line, char });
    }
    advance(k + width);
  }
  return map;
}

/** The source place of rendered character `i`. */
export function sourceAt(map: SourceMap, i: number): SourcePlace {
  let run = map[0] ?? { at: 0, line: 1, char: 0 };
  for (const r of map) {
    if (r.at > i) break;
    run = r;
  }
  return { line: run.line, char: run.char + (i - run.at) };
}

export const before = (a: SourcePlace, b: SourcePlace) =>
  a.line < b.line || (a.line === b.line && a.char < b.char);

/** The rendered characters of a text `length` long whose source lies in
 * `range`, as `[from, to)`. Null when none does. */
export function renderedSlice(
  map: SourceMap,
  length: number,
  range: CommentRange,
): [number, number] | null {
  let from: number | undefined;
  let to = 0;
  for (const [i, run] of map.entries()) {
    const end = run.char + (map[i + 1]?.at ?? length) - run.at;
    const window = rangeSliceOnLine(range, run.line, end);
    if (!window) continue;
    const start = Math.max(window[0], run.char);
    if (start >= window[1]) continue;
    from ??= run.at + start - run.char;
    to = run.at + window[1] - run.char;
  }
  return from === undefined ? null : [from, to];
}

/** The map as a `data-source` attribute: each run as `at:line:char`,
 * separated by spaces. */
export const formatSourceMap = (map: SourceMap) =>
  map.map((r) => `${r.at}:${r.line}:${r.char}`).join(" ");

/** The map in a `data-source` attribute. */
export const parseSourceMap = (text: string): SourceMap =>
  text.split(" ").map((run) => {
    const [at = 0, line = 1, char = 0] = run.split(":").map(Number);
    return { at, line, char };
  });
