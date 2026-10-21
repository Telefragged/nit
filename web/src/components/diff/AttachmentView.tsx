import { attachmentUrl } from "../../api/client";
import type { RenderedKind, ReviewFile } from "../../lib/attachments";
import Markdown from "../Markdown";

/** The rendered body of an attachment, in place of its lines. An HTML
 * page runs in a sandbox with a separate origin, so its scripts cannot call
 * the API. */
export default function AttachmentView({
  file,
  kind,
}: {
  file: ReviewFile;
  kind: RenderedKind;
}) {
  const attachment = file.attachment;
  if (!attachment) return null;
  const url = attachmentUrl(attachment);
  if (kind === "image")
    return (
      <div className="attachment attachment-image">
        <img src={url} alt={attachment.name} />
      </div>
    );
  if (kind === "html")
    return (
      <iframe
        className="attachment attachment-frame"
        sandbox="allow-scripts"
        src={url}
        title={attachment.name}
      />
    );
  const text = file.hunks
    .flatMap((hunk) => hunk.lines)
    .map((line) => line.text)
    .join("\n");
  return (
    <div className="attachment attachment-markdown comment-body">
      <Markdown text={text} />
    </div>
  );
}
