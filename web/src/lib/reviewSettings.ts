import { useCallback, useState, useSyncExternalStore } from "react";
import type { Whitespace } from "../api/types";
import { readSaved, writeSaved } from "./storage";

const KEY = "nit.review-settings";

/** How the reviewer wants a diff drawn. One set per browser, shared by
 * every change the reviewer opens. */
export interface ReviewSettings {
  /** `split` draws the old and the new side in two columns. `auto` draws
   * two columns in a wide window and one column in a narrow window. */
  layout: "unified" | "split" | "auto";
  /** Which threads of earlier revisions are ported into the shown range:
   * `open` the unresolved ones, `all` the resolved ones too. */
  ported: "open" | "all";
  /** `ignore` treats two lines that differ only in whitespace as one line. */
  whitespace: Whitespace;
}

/** The settings of a browser that saved none. */
export const DEFAULTS: ReviewSettings = {
  layout: "unified",
  ported: "open",
  whitespace: "compare",
};

/** Each field falls back on its own, so a value that this build does not
 * accept resets only that field. */
function load(): ReviewSettings {
  const saved = readSaved(KEY) as Partial<ReviewSettings> | null;
  return {
    layout:
      saved?.layout === "split" || saved?.layout === "auto"
        ? saved.layout
        : DEFAULTS.layout,
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

const SPLIT_WINDOW = "(min-width: 1600px)";

function subscribeSplitWindow(onChange: () => void) {
  const query = matchMedia(SPLIT_WINDOW);
  query.addEventListener("change", onChange);
  return () => {
    query.removeEventListener("change", onChange);
  };
}

const isSplitWindow = () => matchMedia(SPLIT_WINDOW).matches;

export function useDiffLayout(
  layout: ReviewSettings["layout"],
): "unified" | "split" {
  const splitWindow = useSyncExternalStore(subscribeSplitWindow, isSplitWindow);
  if (layout !== "auto") return layout;
  return splitWindow ? "split" : "unified";
}
