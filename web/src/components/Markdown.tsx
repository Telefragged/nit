import { memo } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import { highlight } from "../lib/highlight";

const plugins = [remarkGfm, remarkBreaks];

const components: Components = {
  code({ className, children, ...props }) {
    // Fenced blocks carry `language-<tag>`; inline code has none.
    const lang = /language-(\S+)/.exec(className ?? "")?.[1];
    // A fenced block's child is the code text itself; anything else
    // (inline code, nested nodes) renders through the default path.
    if (lang === undefined || typeof children !== "string") {
      return (
        <code className={className} {...props}>
          {children}
        </code>
      );
    }
    // The fence tag is an hljs language name/alias; an unknown tag falls
    // back to escaped plain text inside highlight. Highlight.js escapes
    // its input; nothing user-controlled is injected raw.
    return (
      <code
        className="hljs"
        dangerouslySetInnerHTML={{
          __html: highlight(children, lang),
        }}
      />
    );
  },
  a({ children, ...props }) {
    // A comment link must never navigate away from the review.
    return (
      <a {...props} target="_blank" rel="noopener noreferrer">
        {children}
      </a>
    );
  },
};

/**
 * A comment body rendered as markdown: GFM plus GitHub-style hard line
 * breaks, so pre-markdown bodies keep their line structure. Raw HTML stays
 * escaped (no rehype-raw) — bodies are agent-authored, i.e. untrusted.
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
