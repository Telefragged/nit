import { toJsxRuntime } from "hast-util-to-jsx-runtime";
import { memo, type ReactElement, useMemo } from "react";
import type { Components } from "react-markdown";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
import type { Side } from "../../api/types";
import { type Block, type Part, withClasses } from "../../lib/document";
import { highlight } from "../../lib/highlight";
import type { Press } from "../../lib/useLineDrag";
import { components } from "../markdownElements";
import LineLink from "./LineLink";

/** The components of a comment, and a line of a code block highlighted on
 * its own (lib/document). */
const docComponents: Components = {
  ...components,
  span({ className, children, ...props }) {
    const lang = (props as Record<string, unknown>)["data-lang"];
    if (className !== "code-line" || typeof children !== "string") {
      return (
        <span className={className} {...props}>
          {children}
        </span>
      );
    }
    // Highlight.js escapes its input; nothing user-controlled is injected
    // raw.
    return (
      <span
        className={className}
        {...props}
        dangerouslySetInnerHTML={{
          __html: highlight(children, typeof lang === "string" ? lang : null),
        }}
      />
    );
  },
};

/** One block of a rendered Markdown file, with a link to its lines in
 * the gutter. `tint` is the class suffix that marks the block, for example
 * as hovered. `linked` is the part that the page's link marks: the block
 * itself, or a part inside it. A block that comes from source lines is a
 * part for the `c` key (lib/selection). */
function DocBlock({
  file,
  block,
  side,
  tint,
  linked,
  press,
}: {
  file: string;
  block: Block;
  side: Side;
  tint: string;
  linked: Part | undefined;
  press: Press;
}) {
  // The library types its result as the global `JSX.Element`, which the
  // React 19 types do not declare.
  const content = useMemo(
    () =>
      toJsxRuntime(withClasses(block.node, [[linked?.node, "linked"]]), {
        Fragment,
        jsx,
        jsxs,
        components: docComponents,
        passKeys: true,
      }) as ReactElement,
    [block.node, linked],
  );
  const [start, end] = block.lines;
  const own = linked && !linked.node ? " linked" : "";
  return (
    <div
      className={`doc-block${tint}${own}`}
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
// blocks whose tint or linked part changes.
export default memo(DocBlock);
