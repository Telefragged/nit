// How the review page shows an attachment.

import type { DiffFile, EffectiveAttachment, Line } from "../api/types";
import { ATTACHMENT_PREFIX } from "../api/types";
import { kindOf } from "./rendered";

/** A file the review page shows: a file of the diff, or an attachment
 * the page built from the revision's attachments. */
export type ReviewFile = DiffFile & { attachment?: EffectiveAttachment };

/** Whether the page reads the attachment's text, to show its lines. An
 * image renders from its URL alone. */
export const readsText = (attachment: EffectiveAttachment) =>
  kindOf(attachment.name) !== "image";

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
