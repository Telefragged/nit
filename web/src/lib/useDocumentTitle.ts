import { useEffect } from "react";

/**
 * Names the browser tab after what the page shows: `<context> - nit`.
 *
 * A null context means the page has not loaded the thing it names yet, so
 * the tab keeps the plain product name. The tab returns to that name when
 * the page unmounts, which is what leaves a page that names nothing — the
 * repo list — with a plain `nit`.
 */
export function useDocumentTitle(context: string | null) {
  useEffect(() => {
    if (context === null) return;
    document.title = `${context} - nit`;
    return () => {
      document.title = "nit";
    };
  }, [context]);
}
