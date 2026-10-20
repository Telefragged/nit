import { memo, useSyncExternalStore, type ReactNode } from "react";

import type {
  Anchor,
  CommentInput,
  LogEntry,
  ThreadComment,
  ThreadProjection,
} from "../api/types";
import { verdictStatus } from "../api/verdict";
import { anchorAt, anchorFile, placementLine } from "../lib/comments";
import { displayPath } from "../lib/diffview";
import { shortSha } from "../lib/sha";
import { timeAgo } from "../lib/time";
import type { ChangeLogFeed, Note } from "../lib/useChangeLog";
import { StatusChip } from "./badges";
import { PublishedComment } from "./CommentThread";
import Markdown from "./Markdown";

const firstLine = (text: string) => text.split("\n", 1)[0] ?? "";

/** Whether `text` holds more than its first line. A row summary shows only
 * the first line. */
const hasMore = (text: string) => text.trim() !== firstLine(text).trim();

/** Where an anchor points: `change`, a file, or `file:line`. */
function anchorLabel(anchor: Anchor): string {
  const file = anchorFile(anchor);
  if (file === null) return "change";
  const at = anchorAt(anchor);
  return at === null
    ? displayPath(file)
    : `${displayPath(file)}:${placementLine(at)}`;
}

/** Where a comment lands, and what it does to its thread's resolution. A
 * reply lands on the anchor of its thread. */
function placeLine(c: CommentInput, threads: ThreadProjection[]): string {
  const thread = threads.find((t) => t.id === c.thread_id);
  let place: string;
  if (thread !== undefined) {
    place = `Revision ${thread.revision} ${anchorLabel(thread.anchor)}`;
  } else if (c.thread_id !== null) {
    place = `thread ${c.thread_id}`;
  } else {
    const where = c.anchor === null ? "change" : anchorLabel(c.anchor);
    place = c.revision === null ? where : `Revision ${c.revision} ${where}`;
  }
  if (c.resolved === null) return place;
  return `${place}, ${c.resolved ? "resolved" : "reopened"}`;
}

/** A comment body, drawn as the diff draws a published comment. */
function CommentBox({
  comment,
  thread,
}: {
  comment: ThreadComment;
  thread: number | null;
}) {
  return (
    <div className="log-thread">
      <PublishedComment comment={comment} thread={thread} />
    </div>
  );
}

/** One log row: its kind, the summary line, and the detail it opens to.
 * A row with no detail does not open. */
function Row({
  entry,
  kind = entry.kind,
  summary,
  children,
}: {
  entry: LogEntry;
  kind?: ReactNode;
  summary: ReactNode;
  children?: ReactNode;
}) {
  const line = (
    <>
      <span className="log-kind">{kind}</span>
      <span className="log-summary">{summary}</span>
      <span className="dim log-time" title={entry.created_at}>
        {timeAgo(entry.created_at)}
      </span>
    </>
  );
  return children ? (
    <details className="log-row">
      <summary className="log-line">{line}</summary>
      <div className="log-detail">{children}</div>
    </details>
  ) : (
    <div className="log-row">
      <div className="log-line">{line}</div>
    </div>
  );
}

const NoteRow = memo(function NoteRow({
  note: { entry, revision, reset, tags },
  threads,
}: {
  note: Note;
  threads: ThreadProjection[];
}) {
  switch (entry.kind) {
    case "revision": {
      const p = entry.payload;
      return (
        <Row
          entry={entry}
          kind={`Revision ${revision}`}
          summary={
            <>
              <span className="dim">commit</span>{" "}
              <span className="mono">{shortSha(p.commit_sha)}</span>{" "}
              <span className="dim">parent</span>{" "}
              <span className="mono">{shortSha(p.parent_sha)}</span>
              {reset ? (
                <span className="dim">
                  , status reset to <StatusChip status="pending" />
                </span>
              ) : null}
            </>
          }
        >
          <pre className="log-message dim">
            commit {p.commit_sha}
            {"\n"}parent {p.parent_sha}
          </pre>
          <pre className="log-message">{p.message}</pre>
        </Row>
      );
    }
    case "review": {
      const p = entry.payload;
      return (
        <Row
          entry={entry}
          summary={
            <>
              Revision {p.revision}{" "}
              <StatusChip status={verdictStatus[p.verdict]} />
            </>
          }
        >
          {p.message !== "" || p.comments.length > 0 ? (
            <>
              {p.message !== "" ? (
                <div className="comment-body">
                  <Markdown text={p.message} />
                </div>
              ) : null}
              {p.comments.map((c, i) => (
                <div key={i}>
                  <div className="mono dim">{placeLine(c, threads)}</div>
                  {c.body === "" ? null : (
                    <CommentBox
                      thread={c.thread_id}
                      comment={{
                        body: c.body,
                        review_id: entry.position,
                        created_at: entry.created_at,
                      }}
                    />
                  )}
                </div>
              ))}
            </>
          ) : null}
        </Row>
      );
    }
    case "comment": {
      const p = entry.payload;
      return (
        <Row
          entry={entry}
          summary={<span className="mono">{placeLine(p, threads)}</span>}
        >
          {p.body === "" ? null : (
            <CommentBox
              thread={p.thread_id}
              comment={{
                body: p.body,
                review_id: null,
                created_at: entry.created_at,
              }}
            />
          )}
        </Row>
      );
    }
    case "lifecycle": {
      const p = entry.payload;
      return (
        <Row
          entry={entry}
          summary={
            <>
              {p.action}
              {p.commit_sha ? (
                <span className="mono dim"> {shortSha(p.commit_sha)}</span>
              ) : null}
              {p.message ? ` ${firstLine(p.message)}` : null}
            </>
          }
        >
          {p.message && hasMore(p.message) ? (
            <pre className="log-message">{p.message}</pre>
          ) : null}
        </Row>
      );
    }
    case "tags":
      return (
        <Row
          entry={entry}
          summary={
            <>
              <span className="log-added">+{tags.added.length}</span>{" "}
              <span className="log-removed">−{tags.removed.length}</span> tags
            </>
          }
        >
          {tags.added.length === 0 ? null : (
            <ul className="log-tags mono">
              {tags.removed.map((tag) => (
                <li key={`-${tag}`} className="log-removed">
                  − {tag}
                </li>
              ))}
              {tags.added.map((tag) => (
                <li key={`+${tag}`} className="log-added">
                  + {tag}
                </li>
              ))}
            </ul>
          )}
        </Row>
      );
  }
});

/** A change's log, oldest first, one collapsed row per entry. `threads`
 * names the place each reply lands. */
export default function ChangeLog({
  feed,
  threads,
}: {
  feed: ChangeLogFeed;
  threads: ThreadProjection[];
}) {
  const notes = useSyncExternalStore(feed.subscribe, feed.notes);
  if (notes.length === 0) return null;
  return (
    <section className="change-log">
      <div className="thread-group-title">History</div>
      <div className="log-rows">
        {notes.map((note) => (
          <NoteRow key={note.entry.position} note={note} threads={threads} />
        ))}
      </div>
    </section>
  );
}
