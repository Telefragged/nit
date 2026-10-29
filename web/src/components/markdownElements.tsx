import type { Components } from "react-markdown";
import { Link } from "react-router-dom";
import { highlight } from "../lib/highlight";

/** The path, query and fragment of `href` when it points into this app,
 * else null. */
function localPath(href: string): string | null {
  const url = new URL(href, window.location.href);
  return url.origin === window.location.origin
    ? url.pathname + url.search + url.hash
    : null;
}

/** How comments and documents alike render code fences and links. */
export const components: Components = {
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
