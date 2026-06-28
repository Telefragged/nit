// Mutable store shapes for the mock fixture layer. A change owns its
// revisions, reviews and diffs; chain membership and position are derived
// (walked from parent_sha), not stored on the change.

import type {
  ChangeStatus,
  CommentRange,
  Side,
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
  side: Side;
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
  side: Side;
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
