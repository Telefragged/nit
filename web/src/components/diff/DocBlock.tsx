import { toJsxRuntime } from "hast-util-to-jsx-runtime";
import { memo, type ReactElement, useMemo } from "react";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
import type { Side } from "../../api/types";
import type { Block } from "../../lib/document";
import { plainClick } from "../../lib/shortcutKey";
import { useReview } from "../../pages/reviewContext";
import { components } from "../markdownElements";

/** One block of a rendered Markdown file, with a link to its lines in
 * the gutter. `tint` is the class suffix that marks it, for example as
 * linked. `data-anchor` is the block's comment anchor, which the `c` key
 * reads (lib/selection). */
function DocBlock({
  file,
  block,
  side,
  tint,
}: {
  file: string;
  block: Block;
  side: Side;
  tint: string;
}) {
  const ctx = useReview();
  // The library types its result as the global `JSX.Element`, which the
  // React 19 types do not declare.
  const content = useMemo(
    () =>
      toJsxRuntime(block.node, {
        Fragment,
        jsx,
        jsxs,
        components,
        passKeys: true,
      }) as ReactElement,
    [block.node],
  );
  const [start, end] = block.lines;
  const span = { file, lines: { side, start, end } };
  return (
    <div
      className={`doc-block${tint}`}
      data-side={side}
      data-start={start}
      data-end={end}
      data-anchor={block.at ? JSON.stringify(block.at) : undefined}
    >
      {block.at ? (
        <a
          className="doc-gutter line-link"
          href={ctx.linkHref(span)}
          title={start === end ? `Line ${start}` : `Lines ${start}–${end}`}
          onClick={(e) => {
            if (!plainClick(e)) return;
            e.preventDefault();
            ctx.selectLink(span);
          }}
        >
          {start}
        </a>
      ) : null}
      {content}
    </div>
  );
}

// The parent section re-renders on every hover. memo re-renders only the
// blocks whose tint changes.
export default memo(DocBlock);
