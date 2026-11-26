import { memo } from "react";
import ReactMarkdown from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import { remarkDetails } from "../lib/markdownDetails";
import { components } from "./markdownElements";

const plugins = [remarkGfm, remarkBreaks, remarkDetails];

/**
 * A comment body rendered as markdown: GFM plus GitHub-style hard line
 * breaks, so a plain-text body keeps its line structure. Bodies come from
 * the author, so the only HTML that renders is a collapsible group
 * (`remarkDetails`).
 */
function Markdown({ text }: { text: string }) {
  return (
    <ReactMarkdown remarkPlugins={plugins} components={components}>
      {text}
    </ReactMarkdown>
  );
}

// A body never changes once fetched; memo keeps unrelated ReviewPage
// re-renders (scroll spy, editor state) from re-parsing every visible
// comment through the whole remark pipeline.
export default memo(Markdown);
