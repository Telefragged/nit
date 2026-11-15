import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createDraft } from "../api/client";
import { useReview } from "../pages/reviewContext";
import CommentEditor from "./CommentEditor";

/** The editor of a new thread on the whole change, or on the whole `file`.
 * It saves a draft on the selected revision, then calls `onClose`. */
export default function NewThreadEditor({
  file,
  onClose,
}: {
  file?: string;
  onClose: () => void;
}) {
  const ctx = useReview();
  const queryClient = useQueryClient();
  const create = useMutation({
    mutationFn: (body: string) =>
      createDraft(ctx.changeNumber, {
        thread: {
          anchor:
            file === undefined
              ? { change: { revision: ctx.selected } }
              : { file: { revision: ctx.selected, file } },
          body,
        },
      }),
    onSuccess: () => {
      onClose();
      void queryClient.invalidateQueries({
        queryKey: ["drafts", ctx.changeNumber],
      });
    },
  });
  return (
    <CommentEditor
      placeholder={`Comment on the whole ${file === undefined ? "change" : "file"}…`}
      saving={create.isPending}
      onSave={(body) => {
        create.mutate(body);
      }}
      onCancel={onClose}
    />
  );
}
