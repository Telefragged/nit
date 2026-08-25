import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { createDraft, deleteDraft, updateDraft } from "../api/client";
import type { Draft, ThreadComment } from "../api/types";
import type { UiThread } from "../lib/comments";
import { pendingResolved } from "../lib/comments";
import { timeAgo } from "../lib/time";
import CommentEditor from "./CommentEditor";
import Markdown from "./Markdown";

/** A published comment, read-only: it has no id/state/resolved and is never
 * editable (only the reviewer's own drafts are). */
function PublishedComment({ comment }: { comment: ThreadComment }) {
  const role = comment.review_id !== null ? "reviewer" : "author";
  return (
    <div className="comment">
      <div className="comment-head">
        <span className={`byline byline-${role}`}>{role.toUpperCase()}</span>
        <span className="comment-time">{timeAgo(comment.created_at)}</span>
      </div>
      <div className="comment-body">
        <Markdown text={comment.body} />
      </div>
    </div>
  );
}

/** A pending draft: editable (Edit/Delete), with the DRAFT badge. An
 * empty-body reply draft carries a resolution only — render the intent. */
function DraftComment({
  draft,
  changeNumber,
}: {
  draft: Draft;
  changeNumber: number;
}) {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["drafts", changeNumber] });

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
  // it. A new-thread draft has none.
  const editResolved = draft.thread_id !== null ? draft.resolved : undefined;
  const resolutionOnly = draft.body.trim().length === 0;

  return (
    <div className="comment comment-draft">
      <div className="comment-head">
        <span className="byline byline-reviewer">REVIEWER</span>
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
        <div className="comment-body">
          <Markdown text={draft.body} />
        </div>
      )}
    </div>
  );
}

/** The draft editor a thread opens: `resolved` is the resolve-checkbox
 * default (reply keeps the thread's state, reopen flips it to open), and
 * `isReply` only picks the placeholder. */
interface ThreadEditor {
  isReply: boolean;
  resolved: boolean;
}

/**
 * A comment thread: published comments + pending drafts, with reply / resolve
 * / reopen actions. Resolve is one click — it drafts an empty resolution-only
 * draft directly; reply and reopen open the editor with the resolve checkbox
 * pre-set. The decision is drafted on a reply and applied when the review
 * publishes; the badge shows the pending state. Drafts get dashed chrome via
 * .comment-draft. A draft-only thread (`id === null`) is just its editable
 * draft — no published comments and no actions yet.
 */
export default function CommentThread({
  thread,
  changeNumber,
}: {
  thread: UiThread;
  changeNumber: number;
}) {
  const queryClient = useQueryClient();
  const [editor, setEditor] = useState<ThreadEditor | null>(null);
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["drafts", changeNumber] });

  const resolved = pendingResolved(thread);
  const pending = resolved !== thread.resolved;

  // Reply / resolve / reopen all draft a reply that copies the thread's
  // whole anchor — including its revision, so the copied coordinates stay
  // the ones it was written in (the server's author replies match).
  const saveDraft = useMutation({
    mutationFn: (vars: { body: string; resolved?: boolean }) =>
      createDraft(changeNumber, {
        revision: thread.revision,
        ...(thread.file !== null ? { file: thread.file } : {}),
        side: thread.side,
        ...(thread.range !== null
          ? { at: { selection: thread.range } }
          : thread.line !== null
            ? { at: { whole: thread.line } }
            : {}),
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
    <div className={`thread ${isDraftThread ? "thread-draft" : ""}`}>
      {thread.comments.map((c, i) => (
        <PublishedComment key={i} comment={c} />
      ))}
      {thread.drafts.map((d) => (
        <DraftComment key={d.id} draft={d} changeNumber={changeNumber} />
      ))}
      {editor ? (
        <CommentEditor
          placeholder={editor.isReply ? "Reply…" : "Comment (optional)…"}
          initialResolved={editor.resolved}
          resolvedFrom={resolved}
          saving={saveDraft.isPending}
          onSave={(body, res) => {
            saveDraft.mutate({ body, resolved: res });
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
                  // Resolve needs no justification — fires immediately;
                  // Reopen opens the editor so the reviewer must explain
                  // before it publishes.
                  if (resolved) {
                    setEditor({ isReply: false, resolved: false });
                  } else {
                    saveDraft.mutate({ body: "", resolved: true });
                  }
                }}
                disabled={saveDraft.isPending}
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
