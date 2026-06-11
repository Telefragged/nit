import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  createDraft,
  deleteDraft,
  resolveComment,
  unresolveComment,
  updateDraft,
} from "../api/client";
import type { Comment } from "../api/types";
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
    mutationFn: (body: string) => updateDraft(comment.id, body),
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
            <button className="linkish" onClick={() => setEditing(true)}>
              Edit
            </button>
            <button
              className="linkish linkish-danger"
              onClick={() => remove.mutate()}
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
          saving={update.isPending}
          onSave={(body) => update.mutate(body)}
          onCancel={() => setEditing(false)}
        />
      ) : (
        <div className="comment-body">{comment.body}</div>
      )}
    </div>
  );
}

/**
 * A comment thread: root + replies, resolve toggle (root, reviewer-side),
 * reply-as-draft. Draft members get dashed chrome via .comment-draft.
 */
export default function CommentThread({
  thread,
  changeId,
  draftRevision,
}: {
  thread: Thread;
  changeId: number;
  draftRevision: number;
}) {
  const queryClient = useQueryClient();
  const [replying, setReplying] = useState(false);
  const { root, replies } = thread;
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["change", changeId] });

  const toggleResolve = useMutation({
    mutationFn: () =>
      root.resolved ? unresolveComment(root.id) : resolveComment(root.id),
    onSuccess: invalidate,
  });

  const reply = useMutation({
    mutationFn: (body: string) =>
      createDraft(changeId, {
        revision: draftRevision,
        ...(root.file !== null ? { file: root.file } : {}),
        ...(root.line !== null ? { line: root.line } : {}),
        side: root.side,
        body,
        parent_id: root.id,
      }),
    onSuccess: () => {
      setReplying(false);
      void invalidate();
    },
  });

  const isDraftThread = root.state === "draft";

  return (
    <div
      className={`thread ${isDraftThread ? "thread-draft" : ""} ${
        root.resolved ? "thread-resolved" : ""
      }`}
    >
      <CommentView comment={root} changeId={changeId} />
      {replies.map((r) => (
        <CommentView key={r.id} comment={r} changeId={changeId} />
      ))}
      {replying ? (
        <CommentEditor
          placeholder="Reply…"
          saving={reply.isPending}
          onSave={(body) => reply.mutate(body)}
          onCancel={() => setReplying(false)}
        />
      ) : null}
      {!isDraftThread ? (
        <div className="thread-actions">
          {root.resolved ? (
            <span className="badge badge-green">RESOLVED</span>
          ) : (
            <span className="badge badge-amber">OPEN</span>
          )}
          <span className="spacer" />
          {!replying ? (
            <button className="linkish" onClick={() => setReplying(true)}>
              Reply
            </button>
          ) : null}
          <button
            className="linkish"
            onClick={() => toggleResolve.mutate()}
            disabled={toggleResolve.isPending}
          >
            {root.resolved ? "Reopen" : "Resolve"}
          </button>
        </div>
      ) : null}
    </div>
  );
}
