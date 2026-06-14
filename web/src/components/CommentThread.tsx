import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { createDraft, deleteDraft, updateDraft } from "../api/client";
import type { Comment } from "../api/types";
import { pendingResolved } from "../lib/comments";
import { timeAgo } from "../lib/time";
import CommentEditor from "./CommentEditor";

export interface Thread {
  root: Comment;
  replies: Comment[];
}

function CommentView({
  comment,
  changeId,
}: {
  comment: Comment;
  changeId: number;
}) {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["change", changeId] });

  const update = useMutation({
    mutationFn: (vars: { body: string; resolved?: boolean }) =>
      updateDraft(comment.id, vars),
    onSuccess: () => {
      setEditing(false);
      void invalidate();
    },
  });
  const remove = useMutation({
    mutationFn: () => deleteDraft(comment.id),
    onSuccess: invalidate,
  });

  const isDraft = comment.state === "draft";
  // A reply draft carries a resolve decision; offer the checkbox when editing
  // it. A root/new-comment draft has none (docs/api.md "Thread resolution").
  const editResolved =
    comment.parent_id !== null ? comment.resolved : undefined;
  // An empty-body draft stages a resolution only — render the intent.
  const resolutionOnly = isDraft && comment.body.trim().length === 0;

  return (
    <div className={`comment ${isDraft ? "comment-draft" : ""}`}>
      <div className="comment-head">
        <span className={`author author-${comment.author}`}>
          {comment.author.toUpperCase()}
        </span>
        {isDraft ? <span className="badge badge-amber">DRAFT</span> : null}
        <span className="comment-time">{timeAgo(comment.created_at)}</span>
        {isDraft && !editing ? (
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
          initial={comment.body}
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
          {comment.resolved ? "Resolving this thread" : "Reopening this thread"}
        </div>
      ) : (
        <div className="comment-body">{comment.body}</div>
      )}
    </div>
  );
}

/** The draft editor a thread opens: `resolved` is the resolve-checkbox
 * default (reply keeps the thread's state, resolve/reopen flips it), and
 * `isReply` only picks the placeholder (docs/api.md "Thread resolution"). */
interface ThreadEditor {
  isReply: boolean;
  resolved: boolean;
}

/**
 * A comment thread: root + replies, with reply / resolve / reopen actions
 * that each open the editor with the resolve checkbox pre-set. The decision
 * is staged on a draft reply and applied when the review publishes; the badge
 * shows the pending state. Draft members get dashed chrome via .comment-draft.
 */
export default function CommentThread({
  thread,
  changeId,
}: {
  thread: Thread;
  changeId: number;
}) {
  const queryClient = useQueryClient();
  const [editor, setEditor] = useState<ThreadEditor | null>(null);
  const { root, replies } = thread;
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["change", changeId] });

  // The thread's resolution as it will be after pending drafts publish.
  const resolved = pendingResolved(root, replies);
  const pending = resolved !== root.resolved;

  // Reply / resolve / reopen all stage a draft reply that copies the root's
  // whole anchor — including its revision, so the copied file/line/range stay
  // the coordinates they were written in (the server's agent replies match).
  const stage = useMutation({
    mutationFn: (vars: { body: string; resolved?: boolean }) =>
      createDraft(changeId, {
        revision: root.revision,
        ...(root.file !== null ? { file: root.file } : {}),
        ...(root.line !== null ? { line: root.line } : {}),
        side: root.side,
        ...(root.range !== null ? { range: root.range } : {}),
        body: vars.body,
        parent_id: root.id,
        ...(vars.resolved !== undefined ? { resolved: vars.resolved } : {}),
      }),
    onSuccess: () => {
      setEditor(null);
      void invalidate();
    },
  });

  const isDraftThread = root.state === "draft";

  return (
    <div
      className={`thread ${isDraftThread ? "thread-draft" : ""} ${
        resolved ? "thread-resolved" : ""
      }`}
    >
      <CommentView comment={root} changeId={changeId} />
      {replies.map((r) => (
        <CommentView key={r.id} comment={r} changeId={changeId} />
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
                  setEditor({ isReply: false, resolved: !resolved });
                }}
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
