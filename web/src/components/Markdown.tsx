import { memo } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import { Link } from "react-router-dom";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import { highlight } from "../lib/highlight";

const plugins = [remarkGfm, remarkBreaks];

/** The path, query and fragment of `href` when it points into this app,
 * else null. */
function localPath(href: string): string | null {
  const url = new URL(href, window.location.href);
  return url.origin === window.location.origin
    ? url.pathname + url.search + url.hash
    : null;
}

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
  a({ children, href, ...props }) {
    // A link into nit itself, for example one to a line under review,
    // navigates in this tab. Any other link opens a new tab, so it never
    // navigates away from the review.
    const to = href === undefined ? null : localPath(href);
    if (to !== null) {
      return (
        <Link {...props} to={to}>
          {children}
        </Link>
      );
    }
    return (
      <a {...props} href={href} target="_blank" rel="noopener noreferrer">
        {children}
      </a>
    );
  },
};

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
