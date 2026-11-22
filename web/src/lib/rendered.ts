// How the review page renders a file in place of its lines.

import type { Line, Rect, Side } from "../api/types";
import type { ReviewFile } from "./attachments";
import type { RenderedAnchor, ThreadMark } from "./comments";
import { splitExtension } from "./diffview";

/** How a file renders in place of its lines. */
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

/** The kind of a file, by the extension of its path's last segment. */
const kindOf = (path: string): RenderedKind | undefined =>
  KIND_BY_EXTENSION[
    splitExtension(path.split("/").at(-1) ?? "")[1].toLowerCase()
  ];

/** How `file` renders, by its extension, or null when it shows only as
 * lines or as a binary note.
 *
 * Markdown, and an HTML page of the commit, render from the file's text,
 * so they need lines. An attachment's HTML page or image renders from
 * the attachment's URL. The commit's images have no URL, so they do not
 * render. */
export function renderedKind(file: ReviewFile): RenderedKind | null {
  const kind = kindOf(file.path);
  if (kind === undefined) return null;
  const fromUrl = file.attachment !== undefined && kind !== "markdown";
  return fromUrl || (kind !== "image" && !file.binary) ? kind : null;
}

/** The text of one side of a file, from the whole file as diff lines. */
export const sideText = (lines: readonly Line[], side: Side) =>
  lines
    .flatMap((line) => (line[side] === undefined ? [] : [line.text]))
    .join("\n");

export const renderedSide = (file: ReviewFile): Side =>
  file.status === "deleted" ? "old" : "new";

export const showsAnchor = (kind: RenderedKind, at: RenderedAnchor) =>
  "region" in at ? kind === "image" : kind === "html";

export interface Outline {
  at: RenderedAnchor;
  form: ThreadMark | "linked" | "editing";
  threads: number[];
  active: boolean;
}

export const plainOutline = (
  at: RenderedAnchor,
  form: Outline["form"],
): Outline => ({ at, form, threads: [], active: false });

export function rectBetween(
  a: { x: number; y: number },
  b: { x: number; y: number },
): Rect | null {
  const width = Math.abs(a.x - b.x);
  const height = Math.abs(a.y - b.y);
  return width > 0 && height > 0
    ? { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), width, height }
    : null;
}
