import { useLayoutEffect, type RefObject } from "react";

/**
 * Grow a textarea to fit its content. Collapse to `auto` to measure, then
 * set the height to the scrolled content height. Runs on every `value`
 * change — so newlines expand the box as you type — and on mount, so an
 * editor opened with existing text (editing a comment) fits it from the
 * first frame. Layout effect so the resize happens before paint, with no
 * one-frame jump. The CSS `min-height` stays the floor for short text.
 */
export function useAutosize(
  ref: RefObject<HTMLTextAreaElement | null>,
  value: string,
) {
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    // box-sizing is border-box, but scrollHeight excludes the border, so
    // add it back (offsetHeight − clientHeight is the top+bottom border).
    el.style.height = `${el.scrollHeight + el.offsetHeight - el.clientHeight}px`;
    // ref is a stable useRef object, so value is the only trigger.
  }, [value]);
}
