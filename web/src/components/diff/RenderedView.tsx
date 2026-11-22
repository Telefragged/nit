import { useQuery } from "@tanstack/react-query";
import { type ReactNode, useMemo } from "react";
import { attachmentUrl } from "../../api/client";
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
import HtmlView from "./HtmlView";
import ImageView from "./ImageView";

/** The rendered body of an image or an HTML page, in place of its lines.
 * It renders the whole file, however little of it the diff shows. An
 * HTML page runs in a sandbox with a separate origin, so its scripts
 * cannot call the API. */
export default function RenderedView({
  file,
  kind,
  outlines,
  onLink,
  openEditor,
  reveal,
  panel,
}: {
  file: ReviewFile;
  kind: Exclude<RenderedKind, "markdown">;
  outlines: readonly Outline[];
  onLink: (at: RenderedAnchor | null) => void;
  openEditor: (at: RenderedAnchor) => boolean;
  reveal: string | null;
  panel: { at: RenderedAnchor; content: ReactNode } | null;
}) {
  const ctx = useReview();
  const whole = useQuery({
    ...fileLinesQuery(ctx, file),
    enabled: kind === "html",
  });
  const lines = whole.data?.lines;
  const side = renderedSide(file);
  const text = useMemo(() => lines && sideText(lines, side), [lines, side]);
  const name = displayPath(file.path);
  if (kind === "image")
    return (
      <ImageView
        src={file.attachment && attachmentUrl(file.attachment)}
        alt={name}
        outlines={outlines}
        onLink={onLink}
        panel={panel}
      />
    );
  if (text === undefined)
    return (
      <div className="diff-note">
        {whole.isError ? "The file did not load" : "Loading the file…"}
      </div>
    );
  return (
    <HtmlView
      html={text}
      title={name}
      outlines={outlines}
      onLink={onLink}
      openEditor={openEditor}
      reveal={reveal}
      panel={panel}
    />
  );
}
