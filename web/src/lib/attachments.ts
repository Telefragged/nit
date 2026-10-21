// How the review page shows an attachment.

import type { DiffFile, EffectiveAttachment, Line } from "../api/types";
import { ATTACHMENT_PREFIX } from "../api/types";
import { splitExtension } from "./diffview";

/** A file the review page shows: a file of the diff, or an attachment
 * the page built from the revision's attachments. */
export type ReviewFile = DiffFile & { attachment?: EffectiveAttachment };

/** How an attachment renders in place of lines. */
export type RenderedKind = "markdown" | "html" | "image";

const KIND_BY_EXTENSION: Record<string, RenderedKind> = {
  ".md": "markdown",
  ".markdown": "markdown",
  ".html": "html",
  ".htm": "html",
  ".png": "image",
  ".jpg": "image",
  ".jpeg": "image",
  ".gif": "image",
  ".webp": "image",
  ".avif": "image",
  ".svg": "image",
};

const kindOf = (name: string): RenderedKind | undefined =>
  KIND_BY_EXTENSION[
    splitExtension(name.split("/").at(-1) ?? "")[1].toLowerCase()
  ];

/** Whether the page reads the attachment's text: to show its lines, or
 * to render it as Markdown. An image or an HTML page renders from its
 * URL. */
export const readsText = (attachment: EffectiveAttachment) => {
  const kind = kindOf(attachment.name);
  return kind === undefined || kind === "markdown";
};

/** How an attachment renders, by its name's extension, or null when it
 * shows as a file does: as lines, or as a binary note. Markdown renders
 * from the file's lines, so Markdown without lines shows the binary
 * note. */
export function renderedKind(file: ReviewFile): RenderedKind | null {
  if (!file.attachment) return null;
  const kind = kindOf(file.attachment.name);
  if (kind === undefined || (kind === "markdown" && file.binary)) return null;
  return kind;
}

/** The file the page shows for an attachment. `text` is its content, or
 * null when it is not text. Text shows as one all-added hunk, like a new
 * file, so its lines take comments. */
export function attachmentFile(
  attachment: EffectiveAttachment,
  text: string | null,
): ReviewFile {
  const lines: Line[] = (text ?? "")
    .split("\n")
    .map((line, i) => ({ kind: "add", new: i + 1, text: line }));
  if (text?.endsWith("\n")) lines.pop();
  const shown = text === null ? [] : lines;
  return {
    path: `${ATTACHMENT_PREFIX}${attachment.name}`,
    status: "added",
    attachment,
    binary: text === null,
    additions: shown.length,
    deletions: 0,
    old_total: 0,
    new_total: shown.length,
    hunks:
      shown.length === 0
        ? []
        : [
            {
              old_start: 0,
              old_lines: 0,
              new_start: 1,
              new_lines: shown.length,
              header: "",
              lines: shown,
            },
          ],
  };
}
