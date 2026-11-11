import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import {
  clearDecision,
  setDraftDecision,
  submitDecisions,
} from "../api/client";
import type { ChangeDetail, Decision } from "../api/types";
import { DECISION_LABEL } from "../api/verdict";
import { useAutosize } from "../lib/useAutosize";
import { confirmDiscard } from "../lib/confirmDiscard";
import { shortcutKey } from "../lib/shortcutKey";
import { cn } from "../lib/utils";
import { DraftDecision } from "./badges";
import Modal from "./Modal";
import ThreadCount from "./ThreadCount";
import { Button } from "./ui/button";
import { Kbd } from "./ui/kbd";

/** The decisions that the modal offers, each with the classes of its
 * button. A decision that approves is green and one that rejects is red. */
function offered(abandoned: boolean): { decision: Decision; cls: string }[] {
  const approve = "border-add/55 text-add hover:bg-add-bg";
  return abandoned
    ? [{ decision: "reopen", cls: approve }]
    : [
        { decision: "approve", cls: approve },
        {
          decision: "request_changes",
          cls: "border-del/55 text-del hover:bg-del-bg",
        },
        { decision: "comment", cls: "" },
        {
          decision: "abandon",
          cls: "text-muted-foreground hover:border-del hover:text-del",
        },
      ];
}

/**
 * Slim sticky bottom bar and the review modal it opens (`a`). The modal
 * drafts into the change's `draft_decision`; the bar's **Submit**
 * publishes every listed change's at once.
 * The bar shows the counts of drafts and open threads plus the draft
 * decision so the reviewer can see and submit pending work without leaving
 * the diff.
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
      return submitDecisions({ repo: change.repo_id, tag: [tag] });
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

  // Keyboard twin of the Submit button — same `canSubmit` gate;
  // `shortcutKey` mutes modifier chords and typing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (shortcutKey(e) !== "s" || !canSubmit) return;
      submit.mutate();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [canSubmit, submit]);

  const busy = saveDraft.isPending || clear.isPending;

  // Closing discards only the typed cover message (after confirmation when it
  // diverges from the draft) — the draft decision lives server-side.
  const requestClose = () => {
    if (busy) return;
    const dirty = message.trim() !== (draftDecision?.message ?? "");
    if (!confirmDiscard(dirty, "cover message")) return;
    setError(null);
    onReplyOpenChange(false);
  };

  const stats = (
    <span className="counts review-stats">
      {drafts > 0 ? <ThreadCount mark="draft" count={drafts} word /> : null}
      {unresolved > 0 ? (
        <ThreadCount mark="open" count={unresolved} word />
      ) : null}
      {draftDecision ? (
        <span className="count">
          <DraftDecision decision={draftDecision.decision} /> drafted
        </span>
      ) : (
        <span className="dim">No decision drafted</span>
      )}
      <span className="dim mono">r{selectedRevision}</span>
    </span>
  );

  return (
    <>
      <div className="review-bar">
        {stats}
        <div className="review-bar-actions">
          <Button
            aria-keyshortcuts="a"
            onClick={() => {
              onReplyOpenChange(true);
            }}
          >
            Review <Kbd aria-hidden>a</Kbd>
          </Button>
          <Button
            variant="primary"
            disabled={!canSubmit}
            aria-keyshortcuts="s"
            title={
              drafted === 0
                ? "Draft a decision first (Review)"
                : "Publish every listed change's draft decision"
            }
            onClick={() => {
              submit.mutate();
            }}
          >
            {drafted === 0
              ? "Submit"
              : `Submit ${drafted} review${drafted === 1 ? "" : "s"}`}{" "}
            <Kbd aria-hidden>s</Kbd>
          </Button>
        </div>
      </div>
      {replyOpen ? (
        <Modal
          title="Review"
          aside={stats}
          className="reply-modal w-140"
          onDismiss={requestClose}
          initialFocus={textareaRef}
          actions={
            <>
              <Button variant="ghost" onClick={requestClose} disabled={busy}>
                Cancel
              </Button>
              {draftDecision ? (
                <Button
                  variant="ghost"
                  disabled={busy}
                  onClick={() => {
                    clear.mutate();
                  }}
                >
                  Clear draft
                </Button>
              ) : null}
              <span className="spacer" />
              {offered(abandoned).map(({ decision, cls }) => {
                const drafted = draftDecision?.decision === decision;
                return (
                  <Button
                    key={decision}
                    // The drafted decision has the dashed outline of
                    // unsent work.
                    className={cn(
                      cls,
                      drafted && "border-dashed border-mark-text",
                    )}
                    aria-pressed={drafted}
                    disabled={saveDraft.isPending}
                    title={drafted ? "Currently drafted" : undefined}
                    onClick={() => {
                      saveDraft.mutate(decision);
                    }}
                  >
                    {DECISION_LABEL[decision]}
                  </Button>
                );
              })}
            </>
          }
        >
          <div className="dim">
            Your decision is a draft. Submit publishes the reviews of all listed
            changes at once.
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
        </Modal>
      ) : null}
    </>
  );
}
