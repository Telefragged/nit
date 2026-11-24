import { type ReactNode, useState } from "react";
import { type DocRow, type RenderedDoc, shownRows } from "../../lib/document";
import GapSeparator from "./GapSeparator";

const NO_ROWS: ReadonlySet<number> = new Set();

/** The rows of the rendered Markdown diff `doc`, with each run of
 * unchanged rows far from a change as one separator, like a run of
 * unchanged lines.
 *
 * The hook stores the rows that the buttons of a separator showed, and
 * a new `doc` starts with none. It returns the function that renders
 * the rows: `render` renders one row that shows, and a row for which
 * `keep` is true shows even far from a change. */
export function useDocRows(doc: RenderedDoc | null) {
  const [shown, setShown] = useState<{
    doc: RenderedDoc | null;
    rows: ReadonlySet<number>;
  }>({ doc: null, rows: NO_ROWS });
  const revealed = shown.doc === doc ? shown.rows : NO_ROWS;
  return (
    keep: (row: DocRow, index: number) => boolean,
    render: (row: DocRow, index: number) => ReactNode,
  ): ReactNode[] => {
    if (!doc) return [];
    let gapOrdinal = 0;
    return shownRows(
      doc.rows,
      (row, index) => revealed.has(index) || keep(row, index),
    ).map((item) => {
      if ("row" in item) return render(item.row, item.index);
      const { from, to } = item;
      return (
        <GapSeparator
          key={`gap-ordinal-${gapOrdinal++}`}
          more={to - from + 1}
          unit="block"
          step={1}
          down={from > 0}
          up={to < doc.rows.length - 1}
          reveal={(end, count) => {
            const rows = new Set(revealed);
            const n = Math.min(count, to - from + 1);
            for (let i = 0; i < n; i++)
              rows.add(end === "down" ? from + i : to - i);
            setShown({ doc, rows });
          }}
        />
      );
    });
  };
}
