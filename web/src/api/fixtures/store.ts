// The mutable store shapes behind the wire types — the records the data
// fixtures populate and the server logic mutates in place.
//
// A change owns its revisions, reviews and diffs. It is no longer pinned to
// a chain or a position — those are properties of a derived path. The
// change's displayed status at a revision is derived from `reviews` (the
// verdict of the latest review at that revision), unless `terminal` marks it
// merged/abandoned change-wide.

import type {
  ChangeStatus,
  CommentRange,
  CommentSide,
  Diff,
  Review,
  Revision,
  ThreadComment,
} from "../types";

export interface ChangeRecord {
  id: number;
  repo_id: number;
  change_key: string;
  subject: string;
  /** A terminal change-wide status (merged/abandoned); overrides reviews. */
  terminal?: Extract<ChangeStatus, "merged" | "abandoned">;
  revisions: Revision[];
  reviews: Review[];
  /** Keyed by diffKey(revision, against). */
  diffs: Record<string, Diff>;
}

/** A tip commit: the head of one derived chain. The set of these is the only
 * thing the dashboard enumerates; the path is walked from `parent_sha`. */
export interface TipRecord {
  tip_change_id: number;
  repo_id: number;
  /** The patchset of the tip change this tip pins (its head revision). */
  revision: number;
  /** Sticky; set by push --partial, cleared by ready — on the tip's latest. */
  partial: boolean;
  /** Terminal tips (every member merged/abandoned) — off the dashboard's
   * default `active` view. */
  active: boolean;
}

/** A repo registry entry (docs/api.md "Repos"). */
export interface RepoRecord {
  id: number;
  git_dir: string;
  base_ref: string;
}

/** A published thread (its anchor, rolled-up resolution and conversation) —
 * the mutable store shape behind the wire's Thread. */
export interface ThreadRecord {
  id: number;
  change_id: number;
  revision: number;
  file: string | null;
  line: number | null;
  side: CommentSide;
  /** Selected-text anchor; most fixture threads are whole-line. */
  range?: CommentRange | null;
  line_text: string | null;
  resolved: boolean;
  comments: ThreadComment[];
  created_at: string;
  updated_at: string;
}

/** A reviewer's unpublished comment: a new thread (`thread_id` null) or a
 * reply to a published one (`thread_id` set). */
export interface DraftRecord {
  id: number;
  change_id: number;
  thread_id: number | null;
  revision: number;
  file: string | null;
  line: number | null;
  side: CommentSide;
  range?: CommentRange | null;
  line_text: string | null;
  body: string;
  /** The staged thread-resolution decision. */
  resolved: boolean;
  created_at: string;
  updated_at: string;
}

/** A synthetic canonical-history node (the merged history below HEAD the mock
 * has no git to walk — docs/api.md "Graph"). */
export interface HistNode {
  sha: string;
  subject: string;
  parents: string[];
}
