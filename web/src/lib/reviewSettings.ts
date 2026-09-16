import { useCallback, useState } from "react";
import type { DiffMode } from "../api/types";

const KEY = "nit.review-settings";

/** How the reviewer wants a diff drawn. One set per browser, shared by
 * every change the reviewer opens. */
export interface ReviewSettings {
  /** `outline` collapses every function body, leaving the signatures. */
  mode: DiffMode;
  /** `split` draws the old and the new side in two columns. */
  layout: "unified" | "split";
  /** Which threads of earlier revisions are ported into the shown range:
   * `open` the unresolved ones, `all` the resolved ones too. */
  ported: "open" | "all";
}

const DEFAULTS: ReviewSettings = {
  mode: "full",
  layout: "unified",
  ported: "open",
};

/** Each field falls back on its own, so a value this build does not know
 * costs that one knob rather than the whole page. */
function load(): ReviewSettings {
  let saved: Partial<ReviewSettings> | null = null;
  try {
    saved = JSON.parse(
      localStorage.getItem(KEY) ?? "",
    ) as Partial<ReviewSettings> | null;
  } catch {
    saved = null;
  }
  return {
    mode: saved?.mode === "outline" ? "outline" : DEFAULTS.mode,
    layout: saved?.layout === "split" ? "split" : DEFAULTS.layout,
    ported: saved?.ported === "all" ? "all" : DEFAULTS.ported,
  };
}

/** The saved settings and a writer that persists what it sets. */
export function useReviewSettings(): [
  ReviewSettings,
  (next: ReviewSettings) => void,
] {
  const [settings, setSettings] = useState(load);
  const save = useCallback((next: ReviewSettings) => {
    setSettings(next);
    localStorage.setItem(KEY, JSON.stringify(next));
  }, []);
  return [settings, save];
}
