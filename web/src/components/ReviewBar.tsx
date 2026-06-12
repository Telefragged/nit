import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useLayoutEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ApiError, submitReview } from "../api/client";
import type { Chain, ChangeDetail, Verdict } from "../api/types";
import { confirmDiscard } from "./CommentEditor";

/**
 * Slim sticky bottom bar (counts + a Review button) and the gerrit-style
 * reply modal it opens (also bound to `a` in ReviewPage): cover message,
 * verdict buttons, conflict/error banners. Submitting publishes all drafts
 * atomically (docs/api.md). On a 409 (agent pushed meanwhile) the modal
 * stays open with the cover message and drafts kept, data is refetched and
 * submission is re-offered against the new latest revision.
 */
export default function ReviewBar({
  change,
  chain,
  selectedRevision,
  replyOpen,
  onReplyOpenChange,
}: {
  change: ChangeDetail;
  chain: Chain | undefined;
  selectedRevision: number;
  replyOpen: boolean;
  onReplyOpenChange: (open: boolean) => void;
}) {
  const [message, setMessage] = useState("");
  const [conflict, setConflict] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const drafts = change.comments.filter((c) => c.state === "draft").length;
  const unresolved = change.comments.filter(
    (c) => c.state === "published" && c.parent_id === null && !c.resolved,
  ).length;

  const submit = useMutation({
    mutationFn: (verdict: Verdict) =>
      submitReview(change.id, {
        revision: selectedRevision,
        verdict,
        message: message.trim(),
      }),
    onSuccess: () => {
      setConflict(null);
      setMessage("");
      onReplyOpenChange(false);
      void queryClient.invalidateQueries({ queryKey: ["change", change.id] });
      void queryClient.invalidateQueries({ queryKey: ["chain"] });
      void queryClient.invalidateQueries({ queryKey: ["chains"] });
      // Next pending change in chain order, else back to the chain.
      const next = chain?.changes.find(
        (c) =>
          c.status === "pending" &&
          c.position !== null &&
          change.position !== null &&
          c.position > change.position,
      );
      if (next) {
        navigate(`/changes/${next.id}`);
      } else if (chain) {
        navigate(`/chains/${chain.id}`);
      } else {
        navigate("/");
      }
    },
    onError: (err) => {
      if (err instanceof ApiError && err.status === 409) {
        // Keep message + drafts; refetch so the new revision shows up.
        setConflict(err.message);
        void queryClient.invalidateQueries({ queryKey: ["change", change.id] });
        void queryClient.invalidateQueries({ queryKey: ["diff", change.id] });
      } else {
        setConflict(null);
      }
    },
  });

  // showModal() puts the dialog in the top layer and makes the rest of the
  // page inert: real focus containment behind the dialog role, and Escape
  // arrives as the `cancel` event no matter where focus sits (it can land
  // on <body> after a click on non-focusable dialog text). Layout effect so
  // the dialog is visible the same frame it mounts; focus the textarea
  // explicitly since React's autoFocus fires before showModal opens it.
  useLayoutEffect(() => {
    if (!replyOpen) return;
    dialogRef.current?.showModal();
    textareaRef.current?.focus();
  }, [replyOpen]);

  // Closing only discards the cover message (after confirmation when it
  // would be lost) — drafts live server-side and are kept. Inert while a
  // submit is in flight (like the Cancel button): dismissing then would
  // clear the message while the request still completes and navigates.
  const requestClose = () => {
    if (submit.isPending) return;
    if (!confirmDiscard(message.trim().length > 0, "reply")) return;
    setMessage("");
    setConflict(null);
    submit.reset();
    onReplyOpenChange(false);
  };

  // Draft/unresolved counts + revision, shown in the bar and the modal.
  const stats = (
    <span className="review-stats">
      <span className={drafts > 0 ? "draft-count" : "dim"}>
        {drafts} draft{drafts === 1 ? "" : "s"}
      </span>
      <span className={unresolved > 0 ? "unresolved-count" : "dim"}>
        {unresolved} unresolved
      </span>
      <span className="dim mono">r{selectedRevision}</span>
    </span>
  );

  return (
    <>
      <div className="review-bar">
        {stats}
        <button
          className="btn-primary"
          onClick={() => onReplyOpenChange(true)}
        >
          Review (a)
        </button>
      </div>
      {replyOpen ? (
        <dialog
          ref={dialogRef}
          className="modal-backdrop"
          aria-label="Reply"
          // The full-bleed dialog is its own backdrop, so presses outside
          // the inner box hit the dialog element itself. mousedown, not
          // click: a select-drag started inside the dialog and released on
          // the backdrop must not count as a backdrop click.
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) requestClose();
          }}
          // Escape: route the native close request through confirmDiscard.
          onCancel={(e) => {
            e.preventDefault();
            requestClose();
          }}
          // The browser can still force-close (close-watcher rules let a
          // repeated Escape bypass cancel); sync state so the bar button
          // can reopen — the typed message is kept.
          onClose={() => onReplyOpenChange(false)}
        >
          <div className="reply-modal">
            <div className="reply-modal-head">
              <strong>Reply</strong>
              {stats}
            </div>
            {conflict ? (
              <div className="banner banner-warn review-conflict">
                <strong>new revision landed</strong>
                <span className="banner-body">
                  {conflict} — your drafts and cover message were kept; review
                  the update and submit again.
                </span>
              </div>
            ) : null}
            {submit.isError && !conflict ? (
              <div className="banner banner-error review-conflict">
                <strong>submit failed</strong>
                <span className="banner-body">
                  {submit.error instanceof Error
                    ? submit.error.message
                    : String(submit.error)}
                </span>
              </div>
            ) : null}
            <textarea
              ref={textareaRef}
              placeholder="Cover message (published with the verdict)…"
              value={message}
              onChange={(e) => setMessage(e.target.value)}
            />
            <div className="reply-modal-actions">
              <button onClick={requestClose} disabled={submit.isPending}>
                Cancel
              </button>
              <span className="spacer" />
              <button
                className="btn-approve"
                disabled={submit.isPending}
                onClick={() => submit.mutate("approve")}
              >
                Approve
              </button>
              <button
                className="btn-request"
                disabled={submit.isPending}
                onClick={() => submit.mutate("request_changes")}
              >
                Request changes
              </button>
              <button
                disabled={submit.isPending}
                onClick={() => submit.mutate("comment")}
              >
                Comment
              </button>
            </div>
          </div>
        </dialog>
      ) : null}
    </>
  );
}
