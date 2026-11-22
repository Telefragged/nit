import { useQuery } from "@tanstack/react-query";
import { attachmentUrl } from "../../api/client";
import type { ReactNode } from "react";
import type { ReviewFile } from "../../lib/attachments";
import type { RenderedAnchor } from "../../lib/comments";
import { displayPath } from "../../lib/diffview";
import {
  type Outline,
  renderedSide,
  sideText,
  type RenderedKind,
} from "../../lib/rendered";
import { fileLinesQuery } from "../../lib/useHunkExpansion";
import { useReview } from "../../pages/reviewContext";
import ImageView from "./ImageView";

/** The rendered body of an image or an HTML page, in place of its lines.
 * It renders the whole file, however little of it the diff shows. An
 * HTML page runs in a sandbox with a separate origin, so its scripts
 * cannot call the API. An image shows `outlines` and `panel`. A drag
 * across it calls `onLink` with the rectangle that it covers, and a click
 * without a drag calls it with null. */
export default function RenderedView({
  file,
  kind,
  outlines,
  onLink,
  panel,
}: {
  file: ReviewFile;
  kind: Exclude<RenderedKind, "markdown">;
  outlines: readonly Outline[];
  onLink: (at: RenderedAnchor | null) => void;
  panel: { at: RenderedAnchor; content: ReactNode } | null;
}) {
  const ctx = useReview();
  const url = file.attachment && attachmentUrl(file.attachment);
  const whole = useQuery(fileLinesQuery(ctx, file));
  const name = displayPath(file.path);
  if (kind === "image")
    return (
      <ImageView
        src={url}
        alt={name}
        outlines={outlines}
        onLink={onLink}
        panel={panel}
      />
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
    srcDoc: sideText(lines, renderedSide(file)),
  });
}
