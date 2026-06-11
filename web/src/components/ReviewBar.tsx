import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { ApiError, submitReview } from "../api/client";
import type { Chain, ChangeDetail, Verdict } from "../api/types";

/**
 * Sticky bottom bar: draft/unresolved counts, cover message, verdict
 * buttons. Submits publish all drafts atomically (docs/api.md). On a 409
 * (agent pushed meanwhile) the cover message and drafts are kept, data is
 * refetched and submission is re-offered against the new latest revision.
 */
export default function ReviewBar({
  change,
  chain,
  selectedRevision,
}: {
  change: ChangeDetail;
  chain: Chain | undefined;
  selectedRevision: number;
}) {
  const [message, setMessage] = useState("");
  const [conflict, setConflict] = useState<string | null>(null);
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

  return (
    <div className="review-bar">
      {conflict ? (
        <div className="banner banner-warn review-conflict">
          <strong>new revision landed</strong>
          <span className="banner-body">
            {conflict} — your drafts and cover message were kept; review the
            update and submit again.
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
      <div className="review-bar-controls">
        <span className="stats">
          <span className={drafts > 0 ? "draft-count" : "dim"}>
            {drafts} draft{drafts === 1 ? "" : "s"}
          </span>
          <span className={unresolved > 0 ? "unresolved-count" : "dim"}>
            {unresolved} unresolved
          </span>
          <span className="dim mono">r{selectedRevision}</span>
        </span>
        <input
          type="text"
          placeholder="Cover message (published with the verdict)…"
          value={message}
          onChange={(e) => setMessage(e.target.value)}
        />
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
  );
}
