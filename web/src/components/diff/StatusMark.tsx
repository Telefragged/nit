import { Paperclip } from "lucide-react";
import type { ReviewFile } from "../../lib/attachments";
import { statusLetter } from "../../lib/diffview";

/** A file's status letter, or a paperclip for an attachment. */
export default function StatusMark({ file }: { file: ReviewFile }) {
  return file.attachment ? (
    <Paperclip size={12} aria-label="attachment" />
  ) : (
    statusLetter(file)
  );
}
