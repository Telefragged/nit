import { useQuery } from "@tanstack/react-query";
import { attachmentUrl } from "../../api/client";
import type { ReviewFile } from "../../lib/attachments";
import { displayPath } from "../../lib/diffview";
import { sideText, type RenderedKind } from "../../lib/rendered";
import { fileLinesQuery } from "../../lib/useHunkExpansion";
import { useReview } from "../../pages/reviewContext";

/** The rendered body of an image or an HTML page, in place of its lines.
 * It renders the whole file, however little of it the diff shows. An
 * HTML page runs in a sandbox with a separate origin, so its scripts
 * cannot call the API. */
export default function RenderedView({
  file,
  kind,
}: {
  file: ReviewFile;
  kind: Exclude<RenderedKind, "markdown">;
}) {
  const ctx = useReview();
  const url = file.attachment && attachmentUrl(file.attachment);
  const whole = useQuery(fileLinesQuery(ctx, file));
  const name = displayPath(file.path);
  if (kind === "image")
    return (
      <div className="rendered rendered-image">
        <img src={url} alt={name} />
      </div>
    );
  const frame = (source: { src: string } | { srcDoc: string }) => (
    <iframe
      className="rendered rendered-frame"
      sandbox="allow-scripts"
      title={name}
      {...source}
    />
  );
  if (url !== undefined) return frame({ src: url });
  const lines = whole.data?.lines;
  if (lines === undefined)
    return (
      <div className="diff-note">
        {whole.isError ? "The file did not load" : "Loading the file…"}
      </div>
    );
  return frame({
    srcDoc: sideText(lines, file.status === "deleted" ? "old" : "new"),
  });
}
