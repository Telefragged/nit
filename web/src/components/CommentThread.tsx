import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { createDraft, deleteDraft, updateDraft } from "../api/client";
import type { Draft, ThreadComment } from "../api/types";
import type { UiThread } from "../lib/comments";
import { pendingResolved } from "../lib/comments";
import { timeAgo } from "../lib/time";
import CommentEditor from "./CommentEditor";

/** A published comment, read-only: it has no id/state/resolved and is never
 * editable (only the reviewer's own drafts are). */
function PublishedComment({ comment }: { comment: ThreadComment }) {
  const author = comment.review_id !== null ? "reviewer" : "agent";
  return (
    <div className="comment">
      <div className="comment-head">
        <span className={`author author-${author}`}>
          {author.toUpperCase()}
        </span>
        <span className="comment-time">{timeAgo(comment.created_at)}</span>
      </div>
      <div className="comment-body">{comment.body}</div>
    </div>
  );
}

/** A pending draft: editable (Edit/Delete), with the DRAFT badge. An
 * empty-body reply draft stages a resolution only — render the intent. */
function DraftComment({ draft, changeId }: { draft: Draft; changeId: number }) {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["change", changeId] });

  const update = useMutation({
    mutationFn: (vars: { body: string; resolved?: boolean }) =>
      updateDraft(draft.id, vars),
    onSuccess: () => {
      setEditing(false);
      void invalidate();
    },
  });
  const remove = useMutation({
    mutationFn: () => deleteDraft(draft.id),
    onSuccess: invalidate,
  });

  // A reply draft carries a resolve decision; offer the checkbox when editing
  // it. A new-thread draft has none (docs/api.md "Thread resolution").
  const editResolved = draft.thread_id !== null ? draft.resolved : undefined;
  // An empty-body draft stages a resolution only — render the intent.
  const resolutionOnly = draft.body.trim().length === 0;

  return (
    <div className="comment comment-draft">
      <div className="comment-head">
        <span className="author author-reviewer">REVIEWER</span>
        <span className="badge badge-amber">DRAFT</span>
        <span className="comment-time">{timeAgo(draft.created_at)}</span>
        {!editing ? (
          <span className="comment-tools">
            <button
              className="linkish"
              onClick={() => {
                setEditing(true);
              }}
            >
              Edit
            </button>
            <button
              className="linkish linkish-danger"
              onClick={() => {
                remove.mutate();
              }}
              disabled={remove.isPending}
            >
              Delete
            </button>
          </span>
        ) : null}
      </div>
      {editing ? (
        <CommentEditor
          initial={draft.body}
          initialResolved={editResolved}
          saving={update.isPending}
          onSave={(body, resolved) => {
            update.mutate({ body, resolved });
          }}
          onCancel={() => {
            setEditing(false);
          }}
        />
      ) : resolutionOnly ? (
        <div className="comment-body comment-resolution-only">
          {draft.resolved ? "Resolving this thread" : "Reopening this thread"}
        </div>
      ) : (
        <div className="comment-body">{draft.body}</div>
      )}
    </div>
  );
}

/** The draft editor a thread opens: `resolved` is the resolve-checkbox
 * default (reply keeps the thread's state, reopen flips it to open), and
 * `isReply` only picks the placeholder (docs/api.md "Thread resolution"). */
interface ThreadEditor {
  isReply: boolean;
  resolved: boolean;
}

/**
 * A comment thread: published comments + pending drafts, with reply / resolve
 * / reopen actions. Resolve is one click — it stages an empty resolution-only
 * draft directly; reply and reopen open the editor with the resolve checkbox
 * pre-set. The decision is staged on a draft reply and applied when the review
 * publishes; the badge shows the pending state. Drafts get dashed chrome via
 * .comment-draft. A draft-only thread (`id === null`) is just its editable
 * draft — no published comments and no actions yet.
 */
export default function CommentThread({
  thread,
  changeId,
}: {
  thread: UiThread;
  changeId: number;
}) {
  const queryClient = useQueryClient();
  const [editor, setEditor] = useState<ThreadEditor | null>(null);
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["change", changeId] });

  // The thread's resolution as it will be after pending drafts publish.
  const resolved = pendingResolved(thread);
  const pending = resolved !== thread.resolved;

  // Reply / resolve / reopen all stage a draft reply that copies the thread's
  // whole anchor — including its revision, so the copied file/line/range stay
  // the coordinates they were written in (the server's agent replies match).
  const stage = useMutation({
    mutationFn: (vars: { body: string; resolved?: boolean }) =>
      createDraft(changeId, {
        revision: thread.revision,
        ...(thread.file !== null ? { file: thread.file } : {}),
        ...(thread.line !== null ? { line: thread.line } : {}),
        side: thread.side,
        ...(thread.range !== null ? { range: thread.range } : {}),
        body: vars.body,
        // Always a published thread here — the reply / resolve / reopen
        // actions render only when `thread.id !== null` (the !isDraftThread
        // guard).
        ...(thread.id !== null ? { thread_id: thread.id } : {}),
        ...(vars.resolved !== undefined ? { resolved: vars.resolved } : {}),
      }),
    onSuccess: () => {
      setEditor(null);
      void invalidate();
    },
  });

  const isDraftThread = thread.id === null;

  return (
    <div
      className={`thread ${isDraftThread ? "thread-draft" : ""} ${
        resolved ? "thread-resolved" : ""
      }`}
    >
      {thread.comments.map((c, i) => (
        <PublishedComment key={i} comment={c} />
      ))}
      {thread.drafts.map((d) => (
        <DraftComment key={d.id} draft={d} changeId={changeId} />
      ))}
      {editor ? (
        <CommentEditor
          placeholder={editor.isReply ? "Reply…" : "Comment (optional)…"}
          initialResolved={editor.resolved}
          resolvedFrom={resolved}
          saving={stage.isPending}
          onSave={(body, res) => {
            stage.mutate({ body, resolved: res });
          }}
          onCancel={() => {
            setEditor(null);
          }}
        />
      ) : null}
      {!isDraftThread ? (
        <div className="thread-actions">
          <span className={`badge ${resolved ? "badge-green" : "badge-amber"}`}>
            {resolved ? "RESOLVED" : "OPEN"}
          </span>
          {pending ? (
            <span className="dim" title="applies when you submit the review">
              · unsaved
            </span>
          ) : null}
          <span className="spacer" />
          {editor === null ? (
            <>
              <button
                className="linkish"
                onClick={() => {
                  setEditor({ isReply: true, resolved });
                }}
              >
                Reply
              </button>
              <button
                className="linkish"
                onClick={() => {
                  // Resolve is one click: stage an empty resolution-only draft
                  // straight away. Reopen still opens the editor so the
                  // reviewer can say why before it publishes.
                  if (resolved) {
                    setEditor({ isReply: false, resolved: false });
                  } else {
                    stage.mutate({ body: "", resolved: true });
                  }
                }}
                disabled={stage.isPending}
              >
                {resolved ? "Reopen" : "Resolve"}
              </button>
            </>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
