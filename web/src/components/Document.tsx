import { memo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { components } from "./markdownElements";

const plugins = [remarkGfm];

/**
 * A markdown file rendered as a document: GFM, where a single newline
 * joins two lines of one paragraph. Raw HTML stays escaped, because the
 * file comes from the author.
 */
function Document({ text }: { text: string }) {
  return (
    <ReactMarkdown remarkPlugins={plugins} components={components}>
      {text}
    </ReactMarkdown>
  );
}

// A fetched file never changes. memo keeps a re-render of its section
// from parsing the whole file again.
export default memo(Document);
