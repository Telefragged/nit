import { memo } from "react";
import ReactMarkdown from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import { components } from "./markdownElements";

const plugins = [remarkGfm, remarkBreaks];

/**
 * A comment body rendered as markdown: GFM plus GitHub-style hard line
 * breaks, so pre-markdown bodies keep their line structure. Raw HTML stays
 * escaped (no rehype-raw) — bodies come from the author, i.e. untrusted.
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
