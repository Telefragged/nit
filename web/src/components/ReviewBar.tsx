import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { clearDecision, stageDecision, submitChain } from "../api/client";
import type { Chain, ChangeDetail, Decision } from "../api/types";
import { useAutosize } from "../lib/useAutosize";
import { confirmDiscard } from "../lib/confirmDiscard";
import { isShortcutKey } from "../lib/shortcutKey";

/** Human label for a staged decision (the bar chip + the modal's current state). */
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
 * Slim sticky bottom bar and the review modal it opens (`a`). A decision is
 * drafted, not published: the modal stages a verdict — or an abandon/reopen —
 * into the change's `draft_decision` (docs/api.md "Reviewer decisions"), and
 * the bar's **Submit chain** publishes every member's staged decision at once.
 * The bar shows the draft/unresolved counts plus the staged decision so the
 * reviewer can see and submit pending work without leaving the diff.
 */
export default function ReviewBar({
  change,
  chain,
  memberDecisions,
  selectedRevision,
  unresolved,
  replyOpen,
  onReplyOpenChange,
}: {
  change: ChangeDetail;
  /** The selected revision's chain context, for the chain-wide submit. */
  chain: Chain | undefined;
  /** Each chain member's staged decision (or null), keyed by change id —
   * the source for the chain-wide submit count. */
  memberDecisions: Map<number, Decision | null>;
  selectedRevision: number;
  /** Threads that would stay open once the staged drafts publish. */
  unresolved: number;
  replyOpen: boolean;
  onReplyOpenChange: (open: boolean) => void;
}) {
  const [message, setMessage] = useState("");
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  useAutosize(textareaRef, message);

  const drafts = change.drafts.length;
  const staged = change.draft_decision;
  // This change's path member carries its displayed status (per (change, rev)).
  const here = chain?.path.find((c) => c.change_id === change.id);
  const abandoned = here?.status === "abandoned";
  // Chain members with a staged decision — what Submit publishes.
  const stagedInChain =
    chain?.path.filter(
      (c) => (memberDecisions.get(c.change_id) ?? null) !== null,
    ).length ?? 0;

  const invalidate = () => {
    // The chain-wide count reads every member's staged decision, so refresh all
    // loaded drafts overlays, not only this one (each is keyed ["drafts", id]).
    // The published projection updates itself off the websocket.
    void queryClient.invalidateQueries({ queryKey: ["drafts"] });
    void queryClient.invalidateQueries({ queryKey: ["chain"] });
  };

  // Stage a decision (does not publish); the reviewer sweeps the chain and
  // submits when every member is decided.
  const stage = useMutation({
    mutationFn: (decision: Decision) =>
      stageDecision(change.id, { decision, message: message.trim() }),
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

  // Publish every staged decision in this chain. Best-effort per change: a
  // member skipped for a stale/terminal lifecycle comes back in `errors` and
  // keeps the modal-equivalent banner; a clean run returns to the chain drawer.
  const submit = useMutation({
    mutationFn: () => {
      if (!chain) throw new Error("no chain context to submit");
      return submitChain(chain.tip_change_id, chain.path.at(-1)?.revision);
    },
    onSuccess: (result) => {
      invalidate();
      if (result.errors.length > 0) {
        setError(
          `${result.submitted} submitted; ${result.errors.length} skipped: ` +
            result.errors.map((e) => e.message).join("; "),
        );
      } else if (chain) {
        void navigate(`/repos/${chain.repo_id}#chain-${chain.tip_change_id}`);
      }
    },
    onError: (e) => {
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  // What gates the Submit button and its `s` shortcut alike.
  const canSubmit = stagedInChain > 0 && !submit.isPending;

  // Seed the cover message from the staged decision when the modal opens —
  // adjust-during-render on the false→true edge (not an effect), so the staged
  // text is in the textarea the frame it mounts and no cascading render fires.
  const [wasOpen, setWasOpen] = useState(false);
  if (replyOpen !== wasOpen) {
    setWasOpen(replyOpen);
    if (replyOpen) {
      setMessage(staged?.message ?? "");
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
  // `isShortcutKey` mutes modifiers and typing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (replyOpen || !isShortcutKey(e)) return;
      if (e.key !== "s" || !canSubmit) return;
      submit.mutate();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [replyOpen, canSubmit, submit]);

  // Closing discards only the typed cover message (after confirmation when it
  // diverges from what is staged) — the staged decision lives server-side.
  const requestClose = () => {
    if (stage.isPending || clear.isPending) return;
    const dirty = message.trim() !== (staged?.message ?? "");
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
      {staged ? (
        <span
          className="draft-count"
          title="Your staged decision (not yet submitted)"
        >
          ✎ {DECISION_LABEL[staged.decision]}
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
              stagedInChain === 0
                ? "Stage a decision first (Review)"
                : "Publish every staged decision in this chain"
            }
            onClick={() => {
              submit.mutate();
            }}
          >
            Submit chain (s){stagedInChain > 0 ? ` · ${stagedInChain}` : ""}
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
              Your decision is staged, not published — submit the chain to
              publish every member&apos;s decision at once.
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
                disabled={stage.isPending || clear.isPending}
              >
                Cancel
              </button>
              {staged ? (
                <button
                  className="linkish"
                  disabled={stage.isPending || clear.isPending}
                  onClick={() => {
                    clear.mutate();
                  }}
                >
                  Clear staged
                </button>
              ) : null}
              <span className="spacer" />
              {offered(abandoned).map(({ decision, cls }) => (
                <button
                  key={decision}
                  className={cls}
                  disabled={stage.isPending}
                  title={
                    staged?.decision === decision
                      ? "Currently staged"
                      : undefined
                  }
                  onClick={() => {
                    stage.mutate(decision);
                  }}
                >
                  {staged?.decision === decision ? "✎ " : ""}
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
