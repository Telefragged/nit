import { toJsxRuntime } from "hast-util-to-jsx-runtime";
import { memo, type ReactElement, useMemo } from "react";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
import type { Side } from "../../api/types";
import type { Block } from "../../lib/document";
import type { Press } from "../../lib/useLineDrag";
import { components } from "../markdownElements";
import LineLink from "./LineLink";

/** One block of a rendered Markdown file, with a link to its lines in
 * the gutter. `tint` is the class suffix that marks it, for example as
 * linked. A block that takes comments carries the length of its last line
 * for the `c` key (lib/selection). */
function DocBlock({
  file,
  block,
  side,
  tint,
  press,
}: {
  file: string;
  block: Block;
  side: Side;
  tint: string;
  press: Press;
}) {
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
  return (
    <div
      className={`doc-block${tint}`}
      data-side={side}
      data-start={start}
      data-end={end}
      data-end-char={block.fromSource ? block.endChar : undefined}
    >
      {block.fromSource ? (
        <LineLink
          file={file}
          side={side}
          start={start}
          end={end}
          press={press}
          className=" doc-gutter"
        />
      ) : null}
      {content}
    </div>
  );
}

// The parent section re-renders on every hover. memo re-renders only the
// blocks whose tint changes.
export default memo(DocBlock);
