import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  clearDecision,
  setDraftDecision,
  submitDecisions,
} from "../api/client";
import type { ChangeDetail, Decision } from "../api/types";
import { useAutosize } from "../lib/useAutosize";
import { confirmDiscard } from "../lib/confirmDiscard";
import { shortcutKey } from "../lib/shortcutKey";

/** Human label for a draft decision (the bar chip + the modal's current state). */
const DECISION_LABEL: Record<Decision, string> = {
  approve: "Approve",
  request_changes: "Request changes",
  comment: "Comment",
  abandon: "Abandon",
  reopen: "Reopen",
};

function offered(abandoned: boolean): { decision: Decision; cls: string }[] {
  return abandoned
    ? [{ decision: "reopen", cls: "btn-approve" }]
    : [
        { decision: "approve", cls: "btn-approve" },
        { decision: "request_changes", cls: "btn-request" },
        { decision: "comment", cls: "" },
        { decision: "abandon", cls: "btn-lifecycle" },
      ];
}

/**
 * Slim sticky bottom bar and the review modal it opens (`a`). The modal
 * drafts into the change's `draft_decision`; the bar's **Submit**
 * publishes every listed change's at once.
 * The bar shows the draft/unresolved counts plus the draft decision so the
 * reviewer can see and submit pending work without leaving the diff.
 */
export default function ReviewBar({
  change,
  tag,
  drafted,
  selectedRevision,
  unresolved,
  replyOpen,
  onReplyOpenChange,
}: {
  change: ChangeDetail;
  /** The tag (`key=value`) the listed changes share; undefined when the
   * change carries none, and then nothing submits. */
  tag: string | undefined;
  /** How many changes carrying `tag` have a draft decision — what Submit
   * publishes. */
  drafted: number;
  selectedRevision: number;
  /** Threads that would stay open once the drafts publish. */
  unresolved: number;
  replyOpen: boolean;
  onReplyOpenChange: (open: boolean) => void;
}) {
  const [message, setMessage] = useState("");
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const queryClient = useQueryClient();

  useAutosize(textareaRef, message);

  const drafts = change.drafts.length;
  const draftDecision = change.draft_decision;
  const abandoned = change.revisions.at(-1)?.status === "abandoned";
  const invalidate = () => {
    // The submit count reads every member's draft decision, so refresh all
    // loaded drafts overlays, not only this one (each is keyed ["drafts", id]).
    // The published projection updates itself off the websocket.
    void queryClient.invalidateQueries({ queryKey: ["drafts"] });
  };

  // Draft a decision (does not publish); the reviewer sweeps the listed
  // changes and submits when every one is decided.
  const saveDraft = useMutation({
    mutationFn: (decision: Decision) =>
      setDraftDecision(change.id, { decision, message: message.trim() }),
    onSuccess: () => {
      setError(null);
      onReplyOpenChange(false);
      invalidate();
    },
    onError: (e) => {
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  const clear = useMutation({
    mutationFn: () => clearDecision(change.id),
    onSuccess: () => {
      setError(null);
      onReplyOpenChange(false);
      invalidate();
    },
  });

  // Publish every listed change's draft decision. Best-effort per change: a
  // member skipped for a stale/terminal lifecycle comes back in `errors` and
  // keeps the modal-equivalent banner.
  const submit = useMutation({
    mutationFn: () => {
      if (tag === undefined) throw new Error("the change carries no tag");
      return submitDecisions(change.repo_id, tag);
    },
    onSuccess: (result) => {
      invalidate();
      setError(
        result.errors.length > 0
          ? `${result.submitted} submitted; ${result.errors.length} skipped: ` +
              result.errors.map((e) => e.message).join("; ")
          : null,
      );
    },
    onError: (e) => {
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  // What gates the Submit button and its `s` shortcut alike.
  const canSubmit = drafted > 0 && !submit.isPending;

  // Seed the cover message from the draft decision when the modal opens —
  // adjust-during-render on the false→true edge (not an effect), so the draft
  // text is in the textarea the frame it mounts and no cascading render fires.
  const [wasOpen, setWasOpen] = useState(false);
  if (replyOpen !== wasOpen) {
    setWasOpen(replyOpen);
    if (replyOpen) {
      setMessage(draftDecision?.message ?? "");
      setError(null);
    }
  }

  // showModal() puts the dialog in the top layer and makes the rest of the
  // page inert; Escape arrives as the `cancel` event wherever focus sits. Layout
  // effect so the dialog is visible the frame it mounts; focus the textarea
  // explicitly (React's autoFocus fires before showModal opens it).
  useLayoutEffect(() => {
    if (!replyOpen) return;
    dialogRef.current?.showModal();
    textareaRef.current?.focus();
  }, [replyOpen]);

  // Keyboard twin of the Submit button — same `canSubmit` gate;
  // `shortcutKey` mutes modifier chords and typing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (replyOpen || shortcutKey(e) !== "s" || !canSubmit) return;
      submit.mutate();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [replyOpen, canSubmit, submit]);

  // Closing discards only the typed cover message (after confirmation when it
  // diverges from the draft) — the draft decision lives server-side.
  const requestClose = () => {
    if (saveDraft.isPending || clear.isPending) return;
    const dirty = message.trim() !== (draftDecision?.message ?? "");
    if (!confirmDiscard(dirty, "cover message")) return;
    setError(null);
    onReplyOpenChange(false);
  };

  const stats = (
    <span className="review-stats">
      <span className={drafts > 0 ? "draft-count" : "dim"}>
        {drafts} draft{drafts === 1 ? "" : "s"}
      </span>
      <span className={unresolved > 0 ? "unresolved-count" : "dim"}>
        {unresolved} unresolved
      </span>
      {draftDecision ? (
        <span
          className="draft-count"
          title="Your draft decision (not yet submitted)"
        >
          ✎ {DECISION_LABEL[draftDecision.decision]}
        </span>
      ) : null}
      <span className="dim mono">r{selectedRevision}</span>
    </span>
  );

  return (
    <>
      <div className="review-bar">
        {stats}
        <div className="review-bar-actions">
          <button
            className="btn-primary"
            disabled={!canSubmit}
            title={
              drafted === 0
                ? "Draft a decision first (Review)"
                : "Publish every listed change's draft decision"
            }
            onClick={() => {
              submit.mutate();
            }}
          >
            Submit (s){drafted > 0 ? ` · ${drafted}` : ""}
          </button>
          <button
            className="btn-primary"
            onClick={() => {
              onReplyOpenChange(true);
            }}
          >
            Review (a)
          </button>
        </div>
      </div>
      {replyOpen ? (
        // The native modal dialog is its own full-bleed backdrop; the mousedown
        // below dismisses on a backdrop press. Escape is the keyboard
        // equivalent (onCancel), reachable without a pointer.
        // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- keyboard dismiss is onCancel (Escape)
        <dialog
          ref={dialogRef}
          className="modal-backdrop"
          aria-label="Review"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) requestClose();
          }}
          onCancel={(e) => {
            e.preventDefault();
            requestClose();
          }}
          onClose={() => {
            onReplyOpenChange(false);
          }}
        >
          <div className="reply-modal">
            <div className="reply-modal-head">
              <strong>Review</strong>
              {stats}
            </div>
            <div className="dim reply-modal-hint">
              Your decision is a draft, not published — Submit publishes every
              listed change&apos;s decision at once.
            </div>
            {error ? (
              <div className="banner banner-error review-conflict">
                <strong>action failed</strong>
                <span className="banner-body">{error}</span>
              </div>
            ) : null}
            <textarea
              ref={textareaRef}
              placeholder="Cover message (saved with your decision)…"
              value={message}
              onChange={(e) => {
                setMessage(e.target.value);
              }}
            />
            <div className="reply-modal-actions">
              <button
                onClick={requestClose}
                disabled={saveDraft.isPending || clear.isPending}
              >
                Cancel
              </button>
              {draftDecision ? (
                <button
                  className="linkish"
                  disabled={saveDraft.isPending || clear.isPending}
                  onClick={() => {
                    clear.mutate();
                  }}
                >
                  Clear draft
                </button>
              ) : null}
              <span className="spacer" />
              {offered(abandoned).map(({ decision, cls }) => (
                <button
                  key={decision}
                  className={cls}
                  disabled={saveDraft.isPending}
                  title={
                    draftDecision?.decision === decision
                      ? "Currently drafted"
                      : undefined
                  }
                  onClick={() => {
                    saveDraft.mutate(decision);
                  }}
                >
                  {draftDecision?.decision === decision ? "✎ " : ""}
                  {DECISION_LABEL[decision]}
                </button>
              ))}
            </div>
          </div>
        </dialog>
      ) : null}
    </>
  );
}
