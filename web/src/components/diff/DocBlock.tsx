import { toJsxRuntime } from "hast-util-to-jsx-runtime";
import { memo, type ReactElement, useMemo } from "react";
import type { Components } from "react-markdown";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
import type { CommentRange, Side } from "../../api/types";
import { parseChars } from "../../lib/diffLink";
import {
  type Block,
  MARK_CLASSES,
  markedElement,
  type Part,
} from "../../lib/document";
import { highlight, markTextRange } from "../../lib/highlight";
import type { Press } from "../../lib/useLineDrag";
import { components } from "../markdownElements";
import LineLink from "./LineLink";

const docComponents: Components = {
  ...components,
  span({ className, children, ...props }) {
    if (className !== "code-line" || typeof children !== "string") {
      return (
        <span className={className} {...props}>
          {children}
        </span>
      );
    }
    const data = props as Record<string, unknown>;
    const lang = data["data-lang"];
    const mark = parseChars(String(data["data-mark"]));
    const html = highlight(children, typeof lang === "string" ? lang : null);
    // Highlight.js escapes its input; nothing user-controlled is injected
    // raw.
    return (
      <span
        className={className}
        {...props}
        dangerouslySetInnerHTML={{
          __html: mark
            ? markTextRange(html, mark[0], mark[1], MARK_CLASSES.join(" "))
            : html,
        }}
      />
    );
  },
};

/** One block of a rendered Markdown file, with a link to its lines in
 * the gutter. `change` is the block's class suffix in a diff: added or
 * removed. `linked` is the part to show as linked: the block itself, or
 * a part inside it. `marked` is the part, or the range of text, to show
 * as hovered. A block that comes from source lines is a part for the `c`
 * key (lib/selection). */
function DocBlock({
  file,
  block,
  side,
  change,
  linked,
  marked,
  press,
}: {
  file: string;
  block: Block;
  side: Side;
  change: string;
  linked: Part | undefined;
  marked: Part | CommentRange | undefined;
  press: Press;
}) {
  // The library types its result as the global `JSX.Element`, which the
  // React 19 types do not declare.
  const { content, hovered } = useMemo(() => {
    const { node, hovered } = markedElement(block, linked, marked);
    const content = toJsxRuntime(node, {
      Fragment,
      jsx,
      jsxs,
      components: docComponents,
      passKeys: true,
    }) as ReactElement;
    return { content, hovered };
  }, [block, linked, marked]);
  const [start, end] = block.lines;
  const own =
    (linked && !linked.node ? " linked" : "") +
    (hovered && !hovered.node ? " line-hover" : "");
  return (
    <div
      className={`doc-block${change}${own}`}
      data-side={side}
      data-start={start}
      data-end={end}
      data-part={block.fromSource ? "" : undefined}
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
// blocks whose linked or marked part changes.
export default memo(DocBlock);
