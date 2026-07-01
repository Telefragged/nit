import { useLayoutEffect, type RefObject } from "react";

/**
 * Resizes on mount and on every `value` change, so existing text sizes
 * immediately and typed newlines grow the box live. Uses a layout effect
 * so the resize lands before paint, avoiding a one-frame jump. CSS
 * `min-height` remains the floor for short text.
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
    // ref identity is stable, so value changes are what actually retrigger.
  }, [ref, value]);
}
