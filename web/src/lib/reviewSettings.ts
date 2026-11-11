import { useCallback, useState } from "react";
import type { Whitespace } from "../api/types";
import { readSaved, writeSaved } from "./storage";

const KEY = "nit.review-settings";

/** How the reviewer wants a diff drawn. One set per browser, shared by
 * every change the reviewer opens. */
export interface ReviewSettings {
  /** `split` draws the old and the new side in two columns. */
  layout: "unified" | "split";
  /** Which threads of earlier revisions are ported into the shown range:
   * `open` the unresolved ones, `all` the resolved ones too. */
  ported: "open" | "all";
  /** `ignore` reads two lines that differ only in whitespace as one line. */
  whitespace: Whitespace;
}

/** The settings of a browser that saved none. */
export const DEFAULTS: ReviewSettings = {
  layout: "unified",
  ported: "open",
  whitespace: "compare",
};

/** Each field falls back on its own, so a value this build does not know
 * costs that one knob rather than the whole page. */
function load(): ReviewSettings {
  const saved = readSaved(KEY) as Partial<ReviewSettings> | null;
  return {
    layout: saved?.layout === "split" ? "split" : DEFAULTS.layout,
    ported: saved?.ported === "all" ? "all" : DEFAULTS.ported,
    whitespace: saved?.whitespace === "ignore" ? "ignore" : DEFAULTS.whitespace,
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
    writeSaved(KEY, next);
  }, []);
  return [settings, save];
}
