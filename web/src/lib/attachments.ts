// How the review page shows an attachment.

import type { DiffFile, EffectiveAttachment } from "../api/types";
import { ATTACHMENT_PREFIX } from "../api/types";

/** A file the review page shows: a file of the diff, with the attachment
 * that the file is when it is one. */
export type ReviewFile = DiffFile & { attachment?: EffectiveAttachment };

/** The files of a diff, each attachment file joined to the attachment of
 * `shown` under its name. */
export const reviewFiles = (
  files: readonly DiffFile[],
  shown: readonly EffectiveAttachment[],
): ReviewFile[] =>
  files.map((file) => {
    const attachment = shown.find(
      (a) => ATTACHMENT_PREFIX + a.name === file.path,
    );
    return attachment ? { ...file, attachment } : file;
  });
