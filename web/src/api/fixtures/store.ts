// Mutable store shapes for the mock fixture layer. A change owns its
// revisions, reviews and diffs.

import type {
  Anchor,
  ChangeStatus,
  DiffFile,
  PortedComment,
  Review,
  Revision,
  ThreadComment,
} from "../types";

/** A file authored in ./data may omit either total, which ./index fills
 * from the last hunk when serving. A mock file has no body past its hunks,
 * so it ends where its last hunk does unless it declares otherwise. */
export type AuthoredFile = Omit<DiffFile, "old_total" | "new_total"> & {
  old_total?: number;
  new_total?: number;
};
/** A revision as authored in ./data: its subject and status are folded,
 * never written. */
export type AuthoredRevision = Omit<Revision, "subject" | "status">;
interface AuthoredDiff {
  files: AuthoredFile[];
}

export interface ChangeRecord {
  id: number;
  repo_id: number;
  change_id: string;
  subject: string;
  /** A terminal change-wide status (merged/abandoned); overrides reviews. */
  terminal?: Extract<ChangeStatus, "merged" | "abandoned">;
  /** The tags one `tags` entry put on the change, after its revisions. */
  tags?: Record<string, string>;
  revisions: AuthoredRevision[];
  reviews: Review[];
  /** Keyed by diffKey(revision, against). */
  diffs: Record<string, AuthoredDiff>;
  /** The ported comments the server would compute for each revision,
   * keyed by that revision, resolved threads included: the route drops
   * those unless the request asks for them. Absent means none. */
  ported?: Record<number, PortedComment[]>;
}

/** A repo registry entry. */
export interface RepoRecord {
  id: number;
  git_dir: string;
  canonical_ref: string;
  /** The repo's synthetic canonical history, HEAD-first (the merged history
   * below HEAD the mock has no git to walk). */
  history: HistNode[];
}

/** A published thread (its anchor, rolled-up resolution and conversation) —
 * the mutable store shape behind the wire's Thread. */
export interface ThreadRecord {
  id: number;
  change_number: number;
  revision: number;
  anchor: Anchor;
  resolved: boolean;
  comments: ThreadComment[];
  created_at: string;
  updated_at: string;
}

/** A reviewer's unpublished comment: a new thread (`thread_id` null) or a
 * reply to a published one (`thread_id` set). */
export interface DraftRecord {
  id: number;
  change_number: number;
  thread_id: number | null;
  revision: number;
  anchor: Anchor;
  body: string;
  /** The draft's thread-resolution decision. */
  resolved: boolean;
  created_at: string;
  updated_at: string;
}

/** A synthetic canonical-history node (the merged history below HEAD the mock
 * has no git to walk). `change_id` marks a landed change's commit — the graph
 * enriches the node from the change it names, like the backend's trailer
 * match. */
export interface HistNode {
  sha: string;
  subject: string;
  parents: string[];
  change_id?: string;
}
