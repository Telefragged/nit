import { useEffect, useMemo, useRef, useState } from "react";
import { getFileLines } from "../api/client";
import { COMMIT_MSG_PATH, type DiffFile, type Line } from "../api/types";
import { gapLines } from "./diffview";
import type { ReviewCtx } from "../pages/reviewContext";

/** Lines revealed per click of a context-expand button. */
export const EXPAND_STEP = 10;

/** Context expansion: the whole file
 * is fetched once on the first expand, and the run hidden in each gap
 * is sliced from it and folded back into the bordering hunks as real Lines —
 * drift and all — so highlight/comment/placement and the drift tint flow
 * through unchanged. The synthetic commit message and deletions have no diff to
 * expand. Reveals are keyed by separator `i` (the gap before hunk i): `down`
 * pulls from the gap's top into hunk i-1, `up` from its bottom into hunk i.
 *
 * Returns the spliced `hunks`, whether the file is expandable, the `expand`
 * action, and `busyAt` to check a given end/separator's in-flight state. */
export function useHunkExpansion(file: DiffFile, ctx: ReviewCtx) {
  const expandable = file.path !== COMMIT_MSG_PATH && file.status !== "deleted";
  const [whole, setWhole] = useState<readonly Line[] | null>(null);
  const [down, setDown] = useState<ReadonlyMap<number, number>>(new Map());
  const [up, setUp] = useState<ReadonlyMap<number, number>>(new Map());
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  // Reset when a different diff renders into the same file section (react-query
  // keeps the `file` reference stable while content is unchanged, so a
  // background refetch — e.g. after a comment — keeps what's revealed). Done
  // during render, not in an effect, so stale reveals never paint.
  const [shownFile, setShownFile] = useState(file);
  if (shownFile !== file) {
    setShownFile(file);
    setWhole(null);
    setDown(new Map());
    setUp(new Map());
    setBusy(new Set());
  }
  // The latest committed `file`, so a fetch in flight when the diff switches
  // can detect the switch and drop its result instead of splicing it in.
  const fileRef = useRef(file);
  useEffect(() => {
    fileRef.current = file;
  });
  // The in-flight whole-file fetch, keyed by file so each end's button shares
  // one request and a diff switch starts a fresh one.
  const fetching = useRef<{
    file: DiffFile;
    lines: Promise<readonly Line[] | null>;
  } | null>(null);

  const hunks = useMemo(() => {
    if (!whole || (down.size === 0 && up.size === 0)) return file.hunks;
    const oldN = (ls: Line[]) => ls.filter((l) => l.old !== undefined).length;
    const newN = (ls: Line[]) => ls.filter((l) => l.new !== undefined).length;
    return file.hunks.map((hunk, i) => {
      const upN = up.get(i) ?? 0;
      const before = gapLines(whole, file.hunks[i - 1], hunk);
      const pre = upN > 0 ? before.slice(before.length - upN) : [];
      // `next` is undefined for the last hunk; its down-gap is the run to
      // EOF, which gapLines bounds by the file's end.
      const next = file.hunks[i + 1];
      const downN = down.get(i + 1) ?? 0;
      const post = downN > 0 ? gapLines(whole, hunk, next).slice(0, downN) : [];
      if (pre.length === 0 && post.length === 0) return hunk;
      // A revealed line shifts each side's start/count only where it has a
      // number, so a drift del moves the old side without the new.
      return {
        ...hunk,
        old_start: hunk.old_start - oldN(pre),
        new_start: hunk.new_start - newN(pre),
        old_lines: hunk.old_lines + oldN(pre) + oldN(post),
        new_lines: hunk.new_lines + newN(pre) + newN(post),
        lines: [...pre, ...hunk.lines, ...post],
      };
    });
  }, [file.hunks, whole, down, up]);

  /** The whole file as diff lines, fetched once and shared across both ends
   * and every gap; `null` if the diff switched out from under the fetch. */
  function loadWhole(): Promise<readonly Line[] | null> {
    if (whole) return Promise.resolve(whole);
    if (fetching.current?.file !== file) {
      const lines = getFileLines(
        ctx.changeNumber,
        ctx.selected,
        file,
        ctx.against,
      ).then((r) => {
        if (fileRef.current !== file) return null;
        setWhole(r.lines);
        return r.lines;
      });
      fetching.current = { file, lines };
    }
    return fetching.current.lines;
  }

  /** Reveal the next `count` hidden lines — capped by what the gap before
   * hunk `sep` still hides — at one of its ends. `down` pulls from the
   * gap's top, `up` from its bottom; both
   * walk toward the middle. `sep` past the last hunk is the run to EOF,
   * expanded only from its top (`down`). */
  async function expand(end: "down" | "up", sep: number, count = EXPAND_STEP) {
    const key = `${end}:${sep}`;
    if (busy.has(key)) return;
    setBusy((b) => new Set(b).add(key));
    try {
      const lines = await loadWhole();
      if (!lines || fileRef.current !== file) return;
      const gap = gapLines(lines, file.hunks[sep - 1], file.hunks[sep]);
      const remaining = gap.length - (down.get(sep) ?? 0) - (up.get(sep) ?? 0);
      if (remaining <= 0) return;
      const step = Math.min(count, remaining);
      (end === "down" ? setDown : setUp)((m) =>
        new Map(m).set(sep, (m.get(sep) ?? 0) + step),
      );
    } finally {
      setBusy((b) => {
        const next = new Set(b);
        next.delete(key);
        return next;
      });
    }
  }

  const busyAt = (end: "down" | "up", sep: number) => busy.has(`${end}:${sep}`);

  return { hunks, expandable, expand, busyAt };
}
