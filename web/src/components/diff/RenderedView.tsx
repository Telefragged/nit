import { useQuery } from "@tanstack/react-query";
import { type ComponentProps, useMemo } from "react";
import { attachmentUrl } from "../../api/client";
import type { ReviewFile } from "../../lib/attachments";
import { displayPath } from "../../lib/diffview";
import { renderedSide, sideText, type RenderedKind } from "../../lib/rendered";
import { fileLinesQuery } from "../../lib/useHunkExpansion";
import { useReview } from "../../lib/reviewContext";
import HtmlView from "./HtmlView";
import ImageView from "./ImageView";

type Marks = Omit<ComponentProps<typeof HtmlView>, "html" | "title">;

/** The note in place of a file while its text loads, or after the load
 * failed. */
export function FileLoadNote({ failed }: { failed: boolean }) {
  return (
    <div className="diff-note" aria-busy={!failed}>
      {failed ? "The file did not load" : "Loading the file…"}
    </div>
  );
}

/** An image at `src`, or the HTML page `text` in a sandbox with a
 * separate origin, so its scripts cannot call the API. `text` is
 * undefined while it loads, and `failed` is set when the load failed. */
export function RenderedBody({
  kind,
  name,
  src,
  text,
  failed,
  ...marks
}: {
  kind: Exclude<RenderedKind, "markdown">;
  name: string;
  src: string | undefined;
  text: string | undefined;
  failed: boolean;
} & Marks) {
  if (kind === "image")
    return (
      <ImageView
        src={src}
        alt={name}
        outlines={marks.outlines}
        onLink={marks.onLink}
        panel={marks.panel}
      />
    );
  if (text === undefined) return <FileLoadNote failed={failed} />;
  return <HtmlView html={text} title={name} {...marks} />;
}

/** The rendered body of an image or an HTML page, in place of its lines.
 * It renders the whole file, however little of it the diff shows. */
export default function RenderedView({
  file,
  kind,
  ...marks
}: {
  file: ReviewFile;
  kind: Exclude<RenderedKind, "markdown">;
} & Required<Marks>) {
  const ctx = useReview();
  const whole = useQuery({
    ...fileLinesQuery(ctx, file),
    enabled: kind === "html",
  });
  const lines = whole.data?.lines;
  const side = renderedSide(file);
  const text = useMemo(() => lines && sideText(lines, side), [lines, side]);
  return (
    <RenderedBody
      kind={kind}
      name={displayPath(file.path)}
      src={file.attachment && attachmentUrl(file.attachment)}
      text={text}
      failed={whole.isError}
      {...marks}
    />
  );
}
