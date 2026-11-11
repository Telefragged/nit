import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Check, ChevronDown, ChevronRight } from "lucide-react";
import { useState, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { createDraft, deleteDraft, updateDraft } from "../api/client";
import type { Draft, ThreadComment } from "../api/types";
import type { UiThread } from "../lib/comments";
import { anchorKind, pendingResolved, rangeSince } from "../lib/comments";
import { linksThread } from "../lib/diffLink";
import { timeAgo } from "../lib/time";
import { REVEAL, useReview } from "../pages/reviewContext";
import CommentEditor from "./CommentEditor";
import Markdown from "./Markdown";
import { Button } from "./ui/button";

/** When a comment was written. In a published thread it links to the
 * thread, and following that link scrolls the page to it. */
function CommentTime({ at, thread }: { at: string; thread: number | null }) {
  const { linkHref } = useReview();
  if (thread === null)
    return <span className="comment-time">{timeAgo(at)}</span>;
  return (
    <Link className="comment-time" to={linkHref({ thread })} state={REVEAL}>
      {timeAgo(at)}
    </Link>
  );
}

/** A published comment, read-only: it has no id/state/resolved and is never
 * editable (only the reviewer's own drafts are). `lead` and `tail` are the
 * controls that a thread shows before and after the byline of its first
 * comment. */
export function PublishedComment({
  comment,
  thread,
  lead,
  tail,
}: {
  comment: ThreadComment;
  thread: number | null;
  lead?: ReactNode;
  tail?: ReactNode;
}) {
  const byReviewer = comment.review_id !== null;
  return (
    <div className="comment">
      <div className="comment-head">
        {lead}
        <span className={`byline byline-${byReviewer ? "reviewer" : "author"}`}>
          {byReviewer ? "Reviewer" : "Author"}
        </span>
        <CommentTime at={comment.created_at} thread={thread} />
        {tail}
      </div>
      <div className="comment-body">
        <Markdown text={comment.body} />
      </div>
    </div>
  );
}

/** A pending draft: editable (Edit/Delete), with the Draft label. An
 * empty-body reply draft carries a resolution only — render the intent. */
function DraftComment({
  draft,
  changeNumber,
  thread,
}: {
  draft: Draft;
  changeNumber: number;
  thread: number | null;
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
        <span className="byline byline-reviewer">Reviewer</span>
        <span className="draft-label">Draft</span>
        <CommentTime at={draft.created_at} thread={thread} />
        {!editing ? (
          <span className="comment-tools">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setEditing(true);
              }}
            >
              Edit
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="text-del hover:text-del"
              onClick={() => {
                remove.mutate();
              }}
              disabled={remove.isPending}
            >
              Delete
            </Button>
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

/** Whether a thread is open or resolved once the pending drafts apply.
 * `pending` marks a state that the reviewer drafted and did not submit. */
function ThreadState({
  resolved,
  pending,
}: {
  resolved: boolean;
  pending: boolean;
}) {
  return (
    <span
      className="thread-state"
      title={pending ? "Applies when you submit the review" : undefined}
    >
      {resolved ? (
        <Check size={14} aria-hidden />
      ) : (
        <span className="dot dot-amber" />
      )}
      {resolved ? "Resolved" : "Open"}
      {pending ? ", not submitted" : ""}
    </span>
  );
}

/**
 * A comment thread: published comments + pending drafts, with reply / resolve
 * / reopen actions. Resolve is one click — it drafts an empty resolution-only
 * draft directly; reply and reopen open the editor with the resolve checkbox
 * pre-set. The decision is drafted on a reply and applied when the review
 * publishes; the thread shows the pending state. A draft-only thread
 * (`id === null`) is just its editable draft — no published comments and no
 * actions yet.
 *
 * A published thread shows in full or as one row. It starts as one row when
 * it is resolved, the page's URL does not link to it, and it carries no
 * draft with text.
 * Both forms have the same head: the toggle at its left and the state at
 * its right.
 */
export default function CommentThread({ thread }: { thread: UiThread }) {
  const queryClient = useQueryClient();
  const [editor, setEditor] = useState<ThreadEditor | null>(null);
  const { changeNumber, selected, against, latestRevision, showRange, link } =
    useReview();
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["drafts", changeNumber] });

  const resolved = pendingResolved(thread);
  const pending = resolved !== thread.resolved;

  // A reply reuses the thread's anchor and revision. Its coordinates then
  // match the thread's own, like the server's author replies.
  const saveDraft = useMutation({
    mutationFn: (vars: { body: string; resolved?: boolean }) =>
      createDraft(changeNumber, {
        revision: thread.revision,
        anchor: thread.anchor,
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
  const linked = linksThread(link, thread.id);
  // The reviewer's own choice, once they toggle the thread.
  const [toggled, setToggled] = useState<boolean | null>(null);
  const collapsed =
    toggled ??
    (resolved &&
      !linked &&
      thread.drafts.every((d) => d.body.trim().length === 0));
  const since = rangeSince(thread, selected, against, latestRevision);
  // A ported line thread whose lines the amend rewrote is shown on its file.
  const lineLost =
    thread.ported !== undefined &&
    anchorKind(thread.anchor) === "line" &&
    anchorKind(thread.ported.anchor) !== "line";

  const classes = `thread${isDraftThread ? " thread-draft" : ""}${
    linked ? " thread-linked" : ""
  }`;
  const state = <ThreadState resolved={resolved} pending={pending} />;

  // One control in one place for both forms, so that it does not move when
  // the thread opens or closes.
  const toggle = (
    <Button
      variant="ghost"
      size="icon"
      aria-label={collapsed ? "Expand thread" : "Collapse thread"}
      aria-expanded={!collapsed}
      onClick={() => {
        setToggled(!collapsed);
      }}
    >
      {collapsed ? (
        <ChevronRight size={14} aria-hidden />
      ) : (
        <ChevronDown size={14} aria-hidden />
      )}
    </Button>
  );

  if (!isDraftThread && collapsed) {
    const count = thread.comments.length;
    return (
      <div
        className={`${classes} thread-collapsed`}
        data-thread={thread.id ?? undefined}
      >
        <div className="comment">
          {/* The whole head is a mouse shortcut for the toggle, which is the
              keyboard path. */}
          {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions */}
          <div
            className="comment-head"
            onClick={() => {
              setToggled(false);
            }}
          >
            {toggle}
            <span className="thread-preview">
              {thread.comments[0]?.body.trim().split("\n")[0]}
            </span>
            <span className="thread-count">
              {count} comment{count === 1 ? "" : "s"}
            </span>
            <span className="thread-tail">{state}</span>
          </div>
        </div>
      </div>
    );
  }

  const tail = (
    <span className="thread-tail">
      {thread.ported ? (
        <Button
          variant="ghost"
          size="sm"
          className="italic"
          title="Show the revision this thread was written on"
          onClick={() => {
            showRange({ against: undefined, selected: thread.revision });
          }}
        >
          ported from r{thread.revision}
          {lineLost ? ", line rewritten" : ""}
        </Button>
      ) : null}
      {state}
    </span>
  );

  return (
    <div className={classes} data-thread={thread.id ?? undefined}>
      {thread.comments.map((c, i) => (
        <PublishedComment
          key={i}
          comment={c}
          thread={thread.id}
          lead={i === 0 ? toggle : undefined}
          tail={i === 0 ? tail : undefined}
        />
      ))}
      {thread.drafts.map((d) => (
        <DraftComment
          key={d.id}
          draft={d}
          changeNumber={changeNumber}
          thread={thread.id}
        />
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
      {!isDraftThread && editor === null ? (
        <div className="thread-actions">
          {since ? (
            <Button
              variant="ghost"
              size="sm"
              title="Show what the author changed after this comment"
              onClick={() => {
                showRange(since);
              }}
            >
              Diff against latest
            </Button>
          ) : null}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setEditor({ isReply: true, resolved });
            }}
          >
            Reply
          </Button>
          <Button
            size="sm"
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
          </Button>
        </div>
      ) : null}
    </div>
  );
}
