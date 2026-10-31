import { type RefObject, useCallback, useState } from "react";
import type { Side } from "../api/types";
import {
  type LinkSpan,
  type Lines,
  pressLink,
  sameLink,
  spanOver,
} from "./diffLink";
import { rowLines } from "./selection";
import { useReview } from "../pages/reviewContext";

/** A press on the line number that names `lines` on `side`: one line, or
 * the lines of a rendered block. With `extend`, it stretches the linked
 * span to reach them. */
export type Press = (side: Side, lines: Lines, extend: boolean) => void;

/** The press on a file's line numbers, and the drag that follows it,
 * which link a span of the file's lines.
 *
 * While the button is down, a move over another row of `section` moves
 * the span's far end to that row's lines, and the release links the span.
 * `linked` is the link that the file's rows show: the span of the drag
 * until the page's link holds it, else the page's link. */
export function useLineDrag(
  file: string,
  section: RefObject<HTMLElement | null>,
) {
  const { link, selectLink } = useReview();
  // The page writes the span to the URL only on release, so a drag does
  // not rewrite the history entry once per row. The router reads the new
  // URL in a later render, so the span stays until the link matches it. If
  // the span cleared earlier, the old link would show in between.
  const [drag, setDrag] = useState<LinkSpan | null>(null);
  if (drag && sameLink(link, drag)) setDrag(null);

  const press: Press = useCallback(
    (side, lines, extend) => {
      const pressed = pressLink(link, file, side, lines, extend);
      let span = pressed.span;
      setDrag(span);
      const over = (e: MouseEvent) => {
        if (!(e.target instanceof Element)) return;
        if (!section.current?.contains(e.target)) return;
        const hovered = rowLines(e.target, side);
        if (hovered === null) return;
        const next = spanOver(file, side, pressed.fixed, hovered);
        if (sameLink(next, span)) return;
        span = next;
        setDrag(next);
      };
      document.addEventListener("mouseover", over);
      document.addEventListener(
        "mouseup",
        () => {
          document.removeEventListener("mouseover", over);
          selectLink(span);
        },
        { once: true },
      );
    },
    [link, selectLink, file, section],
  );

  return { linked: drag ?? link, press };
}
