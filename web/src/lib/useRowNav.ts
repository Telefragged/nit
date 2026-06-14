import type { MouseEvent } from "react";
import { useNavigate } from "react-router-dom";

/**
 * Spread props that make a whole table row navigate on click. Clicks that
 * land on a link inside the row are ignored, so anchors keep their native
 * behavior (middle-click / cmd-click open a new tab without also navigating
 * the current one) — no per-anchor stopPropagation needed.
 */
export function useRowNav(to: string) {
  const navigate = useNavigate();
  return {
    onClick: (e: MouseEvent) => {
      if (e.target instanceof Element && e.target.closest("a")) return;
      void navigate(to);
    },
    style: { cursor: "pointer" } as const,
  };
}
