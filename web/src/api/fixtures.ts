// Contract-true canned data + a tiny in-memory implementation of the API
// from docs/api.md. client.ts routes every call here when VITE_MOCK is set,
// so the whole UI (including drafts, resolve, review submission and 409s)
// works without a backend. The data doubles as component-test fixtures.
//
// Chains are DERIVED, never stored: a tip is a (tip_change_id, repo) pair,
// and its path is computed by walking the tip revision's parent_sha back to
// the repo's base through the commit-sha → (change, revision) index (a
// gerrit relation chain — docs/api.md "Chains", docs/data-model.md "Scan
// algorithm"). A change's displayed status is per (change, revision): the
// verdict of the latest review at that revision, else pending (terminal
// merged/abandoned win).
//
// Coverage on purpose:
//   repo 1 (acme-runtime)
//     tip change 12  waiting_for_review — 3 changes; change 11 has 2
//            revisions (amended in place, interdiff available), a resolved
//            thread, an unresolved thread, a thread on a line r1 rewrote
//            (all pinned to r0, so they land on the left of the r0 → r1
//            interdiff), 2 drafts, plus a resolved thread on its commit
//            message (/COMMIT_MSG) and a reworded r1 message so the
//            interdiff carries a real message diff; change 12's diff has a
//            rename and a binary file.
//     tip change 40  merged — only visible via ?status=all.
//   repo 2 (quarry)
//     tip change 20  agents_turn — a changes_requested change, mid-push
//            (partial).
//     tip change 30  approved — single approved change.
//   repo 3 (orbit)  the B-in-two-chains example (docs/api.md): one change
//            (B = 51) reached by two tips at two patchsets — tip C (53) walks
//            B at rev0, tip E (55) walks B at rev1. B's rev0 member carries
//            newer_elsewhere (a newer patchset lives on E's chain) and its
//            rev1 carries merged_elsewhere (a newer revision landed on main);
//            ChangeDetail.chains lists both tips.
//
// Every stored diff leads with the synthetic /COMMIT_MSG file, like the
// real server (docs/api.md "The commit message as a file").

import { ApiError } from "./client";
import { COMMIT_MSG_PATH } from "./types";
import type {
  Chain,
  ChainRef,
  ChainState,
  ChainSummary,
  ChangeDetail,
  ChangeStatus,
  CommentRange,
  CommentSide,
  CreateDraftRequest,
  Decision,
  Diff,
  DiffFile,
  Draft,
  Line,
  PathEntry,
  Repo,
  Review,
  Revision,
  StageDecisionRequest,
  SubmitReviewRequest,
  Thread,
  ThreadComment,
  Verdict,
} from "./types";

// ---------------------------------------------------------------------------
// Helpers

const NOW = Date.now();
const ago = (minutes: number) => new Date(NOW - minutes * 60_000).toISOString();

/** Deterministic fake 40-hex sha from a numeric seed. */
function sha(seed: number): string {
  let x = (seed * 2654435761) >>> 0;
  let out = "";
  for (let i = 0; i < 40; i++) {
    x = (x * 1103515245 + 12345) >>> 0;
    out += ((x >>> 16) % 16).toString(16);
  }
  return out;
}
const short = (full: string) => full.slice(0, 12);

const ctx = (old: number, nw: number, text: string): Line => ({
  kind: "context",
  old,
  new: nw,
  text,
});
const add = (nw: number, text: string): Line => ({
  kind: "add",
  new: nw,
  text,
});
const del = (old: number, text: string): Line => ({ kind: "del", old, text });
/** Mark a line as rebase drift (docs/api.md "Rebase-aware interdiffs"). */
const drift = (line: Line): Line => ({ ...line, drift: true });

/** The /COMMIT_MSG entry of a vs-parent diff: the whole message, all-add. */
function msgFile(message: string): DiffFile {
  const lines = message.replace(/\n$/, "").split("\n");
  return {
    path: COMMIT_MSG_PATH,
    status: "added",
    binary: false,
    additions: lines.length,
    deletions: 0,
    hunks: [
      {
        old_start: 0,
        old_lines: 0,
        new_start: 1,
        new_lines: lines.length,
        header: "",
        lines: lines.map((text, i) => add(i + 1, text)),
      },
    ],
  };
}

// ---------------------------------------------------------------------------
// Mutable store shapes
//
// A change owns its revisions, reviews and diffs. It is no longer pinned to
// a chain or a position — those are properties of a derived path. The
// change's displayed status at a revision is derived from `reviews` (the
// verdict of the latest review at that revision), unless `terminal` marks it
// merged/abandoned change-wide.

interface ChangeRecord {
  id: number;
  repo_id: number;
  change_key: string;
  subject: string;
  last_reviewed_revision: number | null;
  /** A terminal change-wide status (merged/abandoned); overrides reviews. */
  terminal?: Extract<ChangeStatus, "merged" | "abandoned">;
  /** A newer revision of this change landed on the canonical branch; drives
   * `merged_elsewhere` on whichever path member pins an older revision. */
  merged_revision?: number;
  revisions: Revision[];
  reviews: Review[];
  /** Keyed by diffKey(revision, against). */
  diffs: Record<string, Diff>;
}

/** A tip commit: the head of one derived chain. The set of these is the only
 * thing the dashboard enumerates; the path is walked from `parent_sha`. */
interface TipRecord {
  tip_change_id: number;
  repo_id: number;
  /** The patchset of the tip change this tip pins (its head revision). */
  revision: number;
  /** Best-effort name (a branch ref in reality); fixtures store the label. */
  name: string;
  /** Sticky; set by push --partial, cleared by ready — on the tip's latest. */
  partial: boolean;
  /** Terminal tips (every member merged/abandoned) — off the dashboard's
   * default `active` view. */
  active: boolean;
}

/** A repo registry entry (docs/api.md "Repos"). */
interface RepoRecord {
  id: number;
  git_dir: string;
  base_branch: string;
}

/** A published thread (its anchor, rolled-up resolution and conversation) —
 * the mutable store shape behind the wire's {@link Thread}. */
interface ThreadRecord {
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
interface DraftRecord {
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

const diffKey = (revision: number, against?: number) =>
  against === undefined ? `r${revision}` : `r${against}..r${revision}`;

// ---------------------------------------------------------------------------
// Repos

const repos: RepoRecord[] = [
  { id: 1, git_dir: "/home/vetle/src/acme-runtime/.git", base_branch: "main" },
  { id: 2, git_dir: "/home/vetle/src/quarry/.git", base_branch: "main" },
  { id: 3, git_dir: "/home/vetle/src/orbit/.git", base_branch: "main" },
];

// ---------------------------------------------------------------------------
// repo 1 — acme-runtime: feat/token-rotation (tip change 12)

const c10r1 = sha(101);
const c11r1 = sha(111);
const c11r2 = sha(112);
const c12r1 = sha(121);
const parent10 = sha(100); // merge-base on main (not a change)

const msg10r1 =
  "auth: add TokenStore schema and config plumbing\n\n" +
  "Refresh tokens get their own table keyed by token hash, with a\n" +
  "family id so a later change can revoke descendants in one\n" +
  "statement. Config grows [auth.rotation] with a ttl knob.\n\n" +
  "Change-Id: I9a41c7e2b3d4f5a6";

const change10: ChangeRecord = {
  id: 10,
  repo_id: 1,
  change_key: "I9a41c7e2b3d4f5a6",
  subject: "auth: add TokenStore schema and config plumbing",
  last_reviewed_revision: 0,
  revisions: [
    {
      number: 0,
      commit_sha: c10r1,
      short_sha: short(c10r1),
      parent_sha: parent10,
      base_sha: parent10,
      partial: false,
      message: msg10r1,
      created_at: ago(26 * 60),
    },
  ],
  reviews: [
    {
      id: 4,
      revision: 0,
      verdict: "approve",
      message:
        "Schema is right, hash-keyed lookup avoids the timing leak. LGTM.",
      created_at: ago(22 * 60),
    },
  ],
  diffs: {
    [diffKey(0)]: {
      files: [
        msgFile(msg10r1),
        {
          path: "migrations/0004_refresh_tokens.sql",
          status: "added",
          binary: false,
          additions: 9,
          deletions: 0,
          hunks: [
            {
              old_start: 0,
              old_lines: 0,
              new_start: 1,
              new_lines: 9,
              header: "",
              lines: [
                add(1, "CREATE TABLE refresh_tokens ("),
                add(2, "    id         INTEGER PRIMARY KEY,"),
                add(3, "    token_hash TEXT NOT NULL UNIQUE,"),
                add(4, "    family_id  INTEGER NOT NULL,"),
                add(5, "    rotated_at TEXT,"),
                add(6, "    revoked    INTEGER NOT NULL DEFAULT 0,"),
                add(7, "    created_at TEXT NOT NULL"),
                add(8, ");"),
                add(
                  9,
                  "CREATE INDEX idx_tokens_family ON refresh_tokens(family_id);",
                ),
              ],
            },
          ],
        },
        {
          path: "src/config.rs",
          status: "modified",
          binary: false,
          additions: 6,
          deletions: 0,
          hunks: [
            {
              old_start: 31,
              old_lines: 4,
              new_start: 31,
              new_lines: 10,
              header: "pub struct Config",
              lines: [
                ctx(31, 31, "    pub listen: SocketAddr,"),
                ctx(32, 32, "    pub database: PathBuf,"),
                add(33, "    /// Refresh-token rotation policy."),
                add(34, "    #[serde(default)]"),
                add(35, "    pub rotation: RotationConfig,"),
                ctx(33, 36, "}"),
                ctx(34, 37, ""),
                add(38, "fn default_ttl() -> u64 {"),
                add(39, "    14 * 24 * 3600"),
                add(40, "}"),
              ],
            },
          ],
        },
      ],
    },
  },
};

const msg11r1 =
  "auth: rotate refresh tokens on use\n\n" +
  "Every presented refresh token is exchanged for a fresh one and the\n" +
  "old row is marked rotated, so a stolen token stops working the\n" +
  "moment the legitimate client refreshes.\n\n" +
  "Change-Id: I3f2d8a91c0b7e514";
// r1 rewords the message (answering the /COMMIT_MSG thread below), so
// the r0 → r1 interdiff carries a real message diff.
const msg11r2 =
  "auth: rotate refresh tokens on use\n\n" +
  "Every presented refresh token is exchanged for a fresh one and the\n" +
  "old row is marked rotated, so a stolen token stops working the\n" +
  "moment the legitimate client refreshes.\n\n" +
  "Token reuse now revokes the whole family (RFC 6819 §5.2.2.3).\n\n" +
  "Change-Id: I3f2d8a91c0b7e514";

const change11: ChangeRecord = {
  id: 11,
  repo_id: 1,
  change_key: "I3f2d8a91c0b7e514",
  subject: "auth: rotate refresh tokens on use",
  last_reviewed_revision: 0,
  revisions: [
    {
      number: 0,
      commit_sha: c11r1,
      short_sha: short(c11r1),
      parent_sha: c10r1,
      base_sha: parent10,
      partial: false,
      message: msg11r1,
      created_at: ago(25 * 60),
    },
    // r1 is the commit amended in place: same Change-Id, same parent,
    // new sha.
    {
      number: 1,
      commit_sha: c11r2,
      short_sha: short(c11r2),
      parent_sha: c10r1,
      base_sha: parent10,
      partial: false,
      message: msg11r2,
      created_at: ago(95),
    },
  ],
  reviews: [
    {
      id: 5,
      revision: 0,
      verdict: "request_changes",
      message:
        "Rotation flow is right, but the unwrap is a production panic and " +
        "token reuse has to revoke the whole family. Two threads inline.",
      created_at: ago(21 * 60),
    },
  ],
  diffs: {
    // Full diff of revision 0 (parent -> rev0 tree).
    [diffKey(0)]: {
      files: [
        msgFile(msg11r1),
        {
          path: "src/auth/rotate.rs",
          status: "modified",
          binary: false,
          additions: 4,
          deletions: 1,
          hunks: [
            {
              old_start: 18,
              old_lines: 5,
              new_start: 18,
              new_lines: 8,
              header: "impl TokenRotator",
              lines: [
                ctx(18, 18, "impl TokenRotator {"),
                ctx(
                  19,
                  19,
                  "    /// Exchange `presented` for a fresh refresh token.",
                ),
                ctx(
                  20,
                  20,
                  "    pub fn rotate(&self, presented: &str) -> Token {",
                ),
                del(
                  21,
                  "        self.store.swap(presented, Token::generate(&mut self.rng.lock()))",
                ),
                add(
                  21,
                  "        let entry = self.store.lookup(presented).unwrap();",
                ),
                add(
                  22,
                  "        let fresh = Token::generate(&mut self.rng.lock());",
                ),
                add(23, "        self.store.mark_rotated(entry.id, &fresh);"),
                add(24, "        fresh"),
                ctx(22, 25, "    }"),
                ctx(23, 26, "}"),
              ],
            },
          ],
        },
        {
          path: "src/auth/store.rs",
          status: "modified",
          binary: false,
          additions: 5,
          deletions: 0,
          hunks: [
            {
              old_start: 52,
              old_lines: 5,
              new_start: 52,
              new_lines: 10,
              header: "impl TokenStore",
              lines: [
                ctx(52, 52, "impl TokenStore {"),
                ctx(
                  53,
                  53,
                  "    pub fn lookup(&self, raw: &str) -> Option<Entry> {",
                ),
                ctx(
                  54,
                  54,
                  "        self.with_conn(|c| c.query_row(LOOKUP_SQL, [hash(raw)], Entry::from_row).ok())",
                ),
                ctx(55, 55, "    }"),
                add(56, ""),
                add(
                  57,
                  "    pub fn mark_rotated(&self, id: EntryId, next: &Token) {",
                ),
                add(58, "        let conn = self.pool.clone().get();"),
                add(
                  59,
                  "        conn.execute(MARK_SQL, params![id, hash(&next.raw), now()]);",
                ),
                add(60, "    }"),
                ctx(56, 61, "}"),
              ],
            },
          ],
        },
      ],
    },
    // Full diff of revision 1 (parent -> rev1 tree).
    [diffKey(1)]: {
      files: [
        msgFile(msg11r2),
        {
          path: "src/auth/rotate.rs",
          status: "modified",
          binary: false,
          additions: 19,
          deletions: 2,
          hunks: [
            {
              old_start: 18,
              old_lines: 6,
              new_start: 18,
              new_lines: 17,
              header: "impl TokenRotator",
              lines: [
                ctx(18, 18, "impl TokenRotator {"),
                ctx(
                  19,
                  19,
                  "    /// Exchange `presented` for a fresh refresh token.",
                ),
                del(20, "    pub fn rotate(&self, presented: &str) -> Token {"),
                add(
                  20,
                  "    pub fn rotate(&self, presented: &str) -> Result<Token, RotateError> {",
                ),
                del(
                  21,
                  "        self.store.swap(presented, Token::generate(&mut self.rng.lock()))",
                ),
                add(21, "        let entry = self"),
                add(22, "            .store"),
                add(23, "            .lookup(presented)"),
                add(24, "            .ok_or(RotateError::UnknownToken)?;"),
                add(25, "        if entry.rotated_at.is_some() {"),
                add(
                  26,
                  "            // Reuse detected: revoke the whole family (RFC 6819 §5.2.2.3).",
                ),
                add(
                  27,
                  "            self.store.revoke_family(entry.family_id);",
                ),
                add(28, "            return Err(RotateError::ReuseDetected);"),
                add(29, "        }"),
                add(
                  30,
                  "        let fresh = Token::generate(&mut self.rng.lock());",
                ),
                add(31, "        self.store.mark_rotated(entry.id, &fresh);"),
                add(32, "        Ok(fresh)"),
                ctx(22, 33, "    }"),
                ctx(23, 34, "}"),
              ],
            },
            {
              old_start: 40,
              old_lines: 3,
              new_start: 51,
              new_lines: 9,
              header: "pub struct RotationConfig",
              lines: [
                ctx(40, 51, "pub struct RotationConfig {"),
                ctx(41, 52, "    pub ttl: Duration,"),
                ctx(42, 53, "}"),
                add(54, ""),
                add(55, "#[derive(Debug, PartialEq)]"),
                add(56, "pub enum RotateError {"),
                add(57, "    UnknownToken,"),
                add(58, "    ReuseDetected,"),
                add(59, "}"),
              ],
            },
          ],
        },
        {
          path: "src/auth/store.rs",
          status: "modified",
          binary: false,
          additions: 5,
          deletions: 0,
          hunks: [
            {
              old_start: 52,
              old_lines: 5,
              new_start: 52,
              new_lines: 10,
              header: "impl TokenStore",
              lines: [
                ctx(52, 52, "impl TokenStore {"),
                ctx(
                  53,
                  53,
                  "    pub fn lookup(&self, raw: &str) -> Option<Entry> {",
                ),
                ctx(
                  54,
                  54,
                  "        self.with_conn(|c| c.query_row(LOOKUP_SQL, [hash(raw)], Entry::from_row).ok())",
                ),
                ctx(55, 55, "    }"),
                add(56, ""),
                add(
                  57,
                  "    pub fn mark_rotated(&self, id: EntryId, next: &Token) {",
                ),
                add(58, "        let conn = self.pool.clone().get();"),
                add(
                  59,
                  "        conn.execute(MARK_SQL, params![id, hash(&next.raw), now()]);",
                ),
                add(60, "    }"),
                ctx(56, 61, "}"),
              ],
            },
          ],
        },
        {
          path: "tests/rotation.rs",
          status: "added",
          binary: false,
          additions: 16,
          deletions: 0,
          hunks: [
            {
              old_start: 0,
              old_lines: 0,
              new_start: 1,
              new_lines: 16,
              header: "",
              lines: [
                add(1, "use nit_auth::{RotateError, TokenRotator};"),
                add(2, ""),
                add(3, "#[test]"),
                add(4, "fn rotates_fresh_token() {"),
                add(5, "    let (rotator, seeded) = harness();"),
                add(
                  6,
                  '    let next = rotator.rotate(&seeded).expect("first use rotates");',
                ),
                add(7, "    assert_ne!(next.raw, seeded);"),
                add(8, "}"),
                add(9, ""),
                add(10, "#[test]"),
                add(11, "fn reuse_revokes_family() {"),
                add(12, "    let (rotator, seeded) = harness();"),
                add(13, "    let _ = rotator.rotate(&seeded).unwrap();"),
                add(14, "    let err = rotator.rotate(&seeded).unwrap_err();"),
                add(15, "    assert_eq!(err, RotateError::ReuseDetected);"),
                add(16, "}"),
              ],
            },
          ],
        },
      ],
    },
    // Interdiff: effective tree of rev 1 -> effective tree of rev 2,
    // message(1) -> message(2) for /COMMIT_MSG.
    [diffKey(1, 0)]: {
      files: [
        {
          path: COMMIT_MSG_PATH,
          status: "modified",
          binary: false,
          additions: 2,
          deletions: 0,
          hunks: [
            {
              old_start: 4,
              old_lines: 4,
              new_start: 4,
              new_lines: 6,
              header: "",
              lines: [
                ctx(
                  4,
                  4,
                  "old row is marked rotated, so a stolen token stops working the",
                ),
                ctx(5, 5, "moment the legitimate client refreshes."),
                ctx(6, 6, ""),
                add(
                  7,
                  "Token reuse now revokes the whole family (RFC 6819 §5.2.2.3).",
                ),
                add(8, ""),
                ctx(7, 9, "Change-Id: I3f2d8a91c0b7e514"),
              ],
            },
          ],
        },
        {
          path: "src/auth/rotate.rs",
          status: "modified",
          binary: false,
          additions: 17,
          deletions: 3,
          hunks: [
            {
              old_start: 18,
              old_lines: 9,
              new_start: 18,
              new_lines: 17,
              header: "impl TokenRotator",
              lines: [
                ctx(18, 18, "impl TokenRotator {"),
                ctx(
                  19,
                  19,
                  "    /// Exchange `presented` for a fresh refresh token.",
                ),
                del(20, "    pub fn rotate(&self, presented: &str) -> Token {"),
                add(
                  20,
                  "    pub fn rotate(&self, presented: &str) -> Result<Token, RotateError> {",
                ),
                del(
                  21,
                  "        let entry = self.store.lookup(presented).unwrap();",
                ),
                add(21, "        let entry = self"),
                add(22, "            .store"),
                add(23, "            .lookup(presented)"),
                add(24, "            .ok_or(RotateError::UnknownToken)?;"),
                add(25, "        if entry.rotated_at.is_some() {"),
                add(
                  26,
                  "            // Reuse detected: revoke the whole family (RFC 6819 §5.2.2.3).",
                ),
                add(
                  27,
                  "            self.store.revoke_family(entry.family_id);",
                ),
                add(28, "            return Err(RotateError::ReuseDetected);"),
                add(29, "        }"),
                ctx(
                  22,
                  30,
                  "        let fresh = Token::generate(&mut self.rng.lock());",
                ),
                ctx(
                  23,
                  31,
                  "        self.store.mark_rotated(entry.id, &fresh);",
                ),
                del(24, "        fresh"),
                add(32, "        Ok(fresh)"),
                ctx(25, 33, "    }"),
                ctx(26, 34, "}"),
              ],
            },
            {
              old_start: 43,
              old_lines: 3,
              new_start: 51,
              new_lines: 9,
              header: "pub struct RotationConfig",
              lines: [
                ctx(43, 51, "pub struct RotationConfig {"),
                ctx(44, 52, "    pub ttl: Duration,"),
                ctx(45, 53, "}"),
                add(54, ""),
                add(55, "#[derive(Debug, PartialEq)]"),
                add(56, "pub enum RotateError {"),
                add(57, "    UnknownToken,"),
                add(58, "    ReuseDetected,"),
                add(59, "}"),
              ],
            },
          ],
        },
        {
          path: "tests/rotation.rs",
          status: "added",
          binary: false,
          additions: 16,
          deletions: 0,
          hunks: [
            {
              old_start: 0,
              old_lines: 0,
              new_start: 1,
              new_lines: 16,
              header: "",
              lines: [
                add(1, "use nit_auth::{RotateError, TokenRotator};"),
                add(2, ""),
                add(3, "#[test]"),
                add(4, "fn rotates_fresh_token() {"),
                add(5, "    let (rotator, seeded) = harness();"),
                add(
                  6,
                  '    let next = rotator.rotate(&seeded).expect("first use rotates");',
                ),
                add(7, "    assert_ne!(next.raw, seeded);"),
                add(8, "}"),
                add(9, ""),
                add(10, "#[test]"),
                add(11, "fn reuse_revokes_family() {"),
                add(12, "    let (rotator, seeded) = harness();"),
                add(13, "    let _ = rotator.rotate(&seeded).unwrap();"),
                add(14, "    let err = rotator.rotate(&seeded).unwrap_err();"),
                add(15, "    assert_eq!(err, RotateError::ReuseDetected);"),
                add(16, "}"),
              ],
            },
          ],
        },
        // A rebase landed in this file: the agent's real edit (lookup, line
        // 16) sits beside base movement (insert's signature, line 19) that
        // the rebase pulled in. The drift renders contained and grey and is
        // excluded from the counts (additions/deletions are the real edit
        // only) — docs/api.md "Rebase-aware interdiffs".
        {
          path: "src/auth/store.rs",
          status: "modified",
          binary: false,
          additions: 1,
          deletions: 1,
          hunks: [
            {
              old_start: 14,
              old_lines: 8,
              new_start: 14,
              new_lines: 8,
              header: "impl TokenStore",
              lines: [
                ctx(14, 14, "impl TokenStore {"),
                ctx(
                  15,
                  15,
                  "    pub fn lookup(&self, raw: &str) -> Option<&Entry> {",
                ),
                del(16, "        self.by_raw.get(raw)"),
                add(16, "        self.by_raw.get(raw.trim())"),
                ctx(17, 17, "    }"),
                ctx(18, 18, ""),
                drift(del(19, "    pub fn insert(&mut self, e: Entry) {")),
                drift(add(19, "    pub fn insert(&mut self, entry: Entry) {")),
                ctx(20, 20, "        self.dirty = true;"),
                ctx(21, 21, "    }"),
              ],
            },
          ],
        },
      ],
    },
  },
};

const msg12r1 =
  "auth: document rotation and ship flow diagram\n\n" +
  "Renames the stale auth doc and adds the sequence diagram the\n" +
  "incident-review asked for.\n\n" +
  "Change-Id: I77b0e4f5a8123c9d";

const change12: ChangeRecord = {
  id: 12,
  repo_id: 1,
  change_key: "I77b0e4f5a8123c9d",
  subject: "auth: document rotation and ship flow diagram",
  last_reviewed_revision: null,
  revisions: [
    {
      number: 0,
      commit_sha: c12r1,
      short_sha: short(c12r1),
      parent_sha: c11r2,
      base_sha: parent10,
      partial: false,
      message: msg12r1,
      created_at: ago(90),
    },
  ],
  reviews: [],
  diffs: {
    [diffKey(0)]: {
      files: [
        msgFile(msg12r1),
        {
          path: "docs/auth-rotation.md",
          old_path: "docs/auth.md",
          status: "renamed",
          binary: false,
          additions: 7,
          deletions: 2,
          hunks: [
            {
              old_start: 1,
              old_lines: 5,
              new_start: 1,
              new_lines: 10,
              header: "",
              lines: [
                del(1, "# Auth"),
                add(1, "# Refresh-token rotation"),
                ctx(2, 2, ""),
                del(3, "TODO: describe the token flow."),
                add(
                  3,
                  "Every presented refresh token is single-use. On use the server",
                ),
                add(
                  4,
                  "issues a successor in the same *family*; presenting a token that",
                ),
                add(5, "was already rotated revokes the entire family."),
                add(6, ""),
                add(7, "![rotation flow](../assets/rotation-flow.png)"),
                add(8, ""),
                ctx(4, 9, "## Endpoints"),
                ctx(5, 10, ""),
              ],
            },
          ],
        },
        {
          path: "assets/rotation-flow.png",
          status: "added",
          binary: true,
          additions: 0,
          deletions: 0,
          hunks: [],
        },
      ],
    },
  },
};

// ---------------------------------------------------------------------------
// repo 1 — build/rustls (tip change 40, merged; only with ?status=all)

const c40r1 = sha(401);

const msg40r1 =
  "build: drop unused openssl feature\n\nChange-Id: I0d9c8b7a6f5e4321";

const change40: ChangeRecord = {
  id: 40,
  repo_id: 1,
  change_key: "I0d9c8b7a6f5e4321",
  subject: "build: drop unused openssl feature",
  last_reviewed_revision: 0,
  terminal: "merged",
  revisions: [
    {
      number: 0,
      commit_sha: c40r1,
      short_sha: short(c40r1),
      parent_sha: sha(400),
      base_sha: sha(400),
      partial: false,
      message: msg40r1,
      created_at: ago(4 * 24 * 60),
    },
  ],
  reviews: [
    {
      id: 8,
      revision: 0,
      verdict: "approve",
      message: "",
      created_at: ago(3 * 24 * 60),
    },
  ],
  diffs: {
    [diffKey(0)]: {
      files: [
        msgFile(msg40r1),
        {
          path: "Cargo.toml",
          status: "modified",
          binary: false,
          additions: 1,
          deletions: 1,
          hunks: [
            {
              old_start: 14,
              old_lines: 3,
              new_start: 14,
              new_lines: 3,
              header: "[dependencies]",
              lines: [
                ctx(14, 14, 'serde = { version = "1", features = ["derive"] }'),
                del(
                  15,
                  'reqwest = { version = "0.12", features = ["native-tls"] }',
                ),
                add(
                  15,
                  'reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }',
                ),
                ctx(16, 16, 'tokio = { version = "1", features = ["full"] }'),
              ],
            },
          ],
        },
      ],
    },
  },
};

// ---------------------------------------------------------------------------
// repo 2 — quarry: fix/wal-checkpoint (tip change 20, agents_turn, partial)

const c20r1 = sha(201);
const parent20 = sha(200);

const msg20r1 =
  "wal: checkpoint on idle, not on every commit\n\n" +
  "Checkpointing after each commit stalls writers; move it to the idle\n" +
  "loop with a 4MiB backlog threshold.\n\n" +
  "Change-Id: Ib8d3e6f1a4c75290";

const change20: ChangeRecord = {
  id: 20,
  repo_id: 2,
  change_key: "Ib8d3e6f1a4c75290",
  subject: "wal: checkpoint on idle, not on every commit",
  last_reviewed_revision: 0,
  revisions: [
    {
      number: 0,
      commit_sha: c20r1,
      short_sha: short(c20r1),
      parent_sha: parent20,
      base_sha: parent20,
      partial: true,
      message: msg20r1,
      created_at: ago(8 * 60),
    },
  ],
  reviews: [
    {
      id: 6,
      revision: 0,
      verdict: "request_changes",
      message:
        "Threshold needs to be configurable and the deleted backoff still " +
        "had one caller.",
      created_at: ago(3 * 60),
    },
  ],
  diffs: {
    [diffKey(0)]: {
      files: [
        msgFile(msg20r1),
        {
          path: "src/wal.rs",
          status: "modified",
          binary: false,
          additions: 6,
          deletions: 3,
          hunks: [
            {
              old_start: 88,
              old_lines: 7,
              new_start: 88,
              new_lines: 10,
              header: "fn commit",
              lines: [
                ctx(88, 88, "        self.append(frame)?;"),
                del(
                  89,
                  "        self.checkpoint()?; // stalls every writer behind fsync",
                ),
                ctx(90, 89, "        Ok(seq)"),
                ctx(91, 90, "    }"),
                ctx(92, 91, ""),
                add(
                  92,
                  "    /// Called from the idle loop; cheap no-op below the backlog threshold.",
                ),
                add(
                  93,
                  "    pub fn maybe_checkpoint(&self) -> io::Result<()> {",
                ),
                add(
                  94,
                  "        if self.backlog_bytes() < CHECKPOINT_BACKLOG {",
                ),
                add(95, "            return Ok(());"),
                add(96, "        }"),
                add(97, "        self.checkpoint()"),
                ctx(93, 98, "    }"),
                del(94, "    fn backoff(&self) -> Duration {"),
                del(
                  95,
                  "        Duration::from_millis(2u64.pow(self.retries.min(6)))",
                ),
              ],
            },
          ],
        },
        {
          path: "src/wal/backoff.rs",
          status: "deleted",
          binary: false,
          additions: 0,
          deletions: 5,
          hunks: [
            {
              old_start: 1,
              old_lines: 5,
              new_start: 0,
              new_lines: 0,
              header: "",
              lines: [
                del(1, "use std::time::Duration;"),
                del(2, ""),
                del(3, "pub fn jitter(base: Duration) -> Duration {"),
                del(4, "    base.mul_f64(0.5 + fastrand::f64())"),
                del(5, "}"),
              ],
            },
          ],
        },
      ],
    },
  },
};

// ---------------------------------------------------------------------------
// repo 2 — quarry: chore/dedupe-ci-cache (tip change 30, approved)

const c30r1 = sha(301);

const msg30r1 =
  "ci: key caches on lockfile hash only\n\n" +
  "Keying on the branch name made every PR start cold.\n\n" +
  "Change-Id: Ie1f4a7b2c5d80936";

const change30: ChangeRecord = {
  id: 30,
  repo_id: 2,
  change_key: "Ie1f4a7b2c5d80936",
  subject: "ci: key caches on lockfile hash only",
  last_reviewed_revision: 0,
  revisions: [
    {
      number: 0,
      commit_sha: c30r1,
      short_sha: short(c30r1),
      parent_sha: sha(300),
      base_sha: sha(300),
      partial: false,
      message: msg30r1,
      created_at: ago(50 * 60),
    },
  ],
  reviews: [
    {
      id: 7,
      revision: 0,
      verdict: "approve",
      message: "Nice catch.",
      created_at: ago(40 * 60),
    },
  ],
  diffs: {
    [diffKey(0)]: {
      files: [
        msgFile(msg30r1),
        {
          path: ".github/workflows/ci.yml",
          status: "modified",
          binary: false,
          additions: 1,
          deletions: 1,
          hunks: [
            {
              old_start: 24,
              old_lines: 3,
              new_start: 24,
              new_lines: 3,
              header: "jobs.test.steps",
              lines: [
                ctx(24, 24, "      - uses: actions/cache@v4"),
                ctx(25, 25, "        with:"),
                del(
                  26,
                  "          key: cargo-${{ github.ref }}-${{ hashFiles('Cargo.lock') }}",
                ),
                add(26, "          key: cargo-${{ hashFiles('Cargo.lock') }}"),
                ctx(27, 27, "          path: ~/.cargo"),
              ],
            },
          ],
        },
      ],
    },
  },
};

// ---------------------------------------------------------------------------
// repo 3 — orbit: the B-in-two-chains example (docs/api.md "Chains")
//
//   push 1:  m → A(50) → B(51) → C(53)      Change-Ids Ia, Ib, Ic
//   push 2:  m → D(52) → B′(51) → E(55)     Change-Ids Id, Ib, Ie
//
// B is one change (51) with two patchsets: rev0 parent=A, rev1 parent=D.
// Two tips, two chains: chains/53 walks B at rev0, chains/55 walks B at rev1.
// Threads/reviews on B are shared (they belong to the change), each anchored
// to the revision it was written against. Revisions are 0-based here (the new
// API), so this scenario exercises rev0 / rev1 display directly.

const mOrbit = sha(500); // merge-base on main
const cA = sha(501);
const cB0 = sha(510); // B rev0 (parent A)
const cB1 = sha(511); // B rev1 (parent D)
const cC = sha(530);
const cD = sha(520);
const cE = sha(550);

const msgA =
  "orbit: extract the scheduler trait\n\n" +
  "Pulls the run-queue behind a Scheduler trait so the fair and the\n" +
  "deadline policies can share a driver.\n\n" +
  "Change-Id: Iaa11bb22cc33dd44";
const msgD =
  "orbit: add a deadline clock source\n\n" +
  "A monotonic clock the deadline scheduler reads; injectable in tests.\n\n" +
  "Change-Id: Idd44cc33bb22aa11";
const msgB =
  "orbit: fair-share scheduler policy\n\n" +
  "Weights each task by its recent CPU share and picks the lightest,\n" +
  "so a busy task can't starve the queue.\n\n" +
  "Change-Id: Ibb22cc33dd44ee55";
const msgC =
  "orbit: wire the fair policy into the runtime\n\n" +
  "Change-Id: Icc33dd44ee55ff66";
const msgE =
  "orbit: deadline policy on top of fair-share\n\n" +
  "Change-Id: Iee55ff66aa11bb22";

function trivialDiff(message: string, path: string, line: string): Diff {
  return {
    files: [
      msgFile(message),
      {
        path,
        status: "modified",
        binary: false,
        additions: 1,
        deletions: 0,
        hunks: [
          {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 2,
            header: "",
            lines: [ctx(1, 1, "// orbit"), add(2, line)],
          },
        ],
      },
    ],
  };
}

const changeA: ChangeRecord = {
  id: 50,
  repo_id: 3,
  change_key: "Iaa11bb22cc33dd44",
  subject: "orbit: extract the scheduler trait",
  last_reviewed_revision: 0,
  revisions: [
    {
      number: 0,
      commit_sha: cA,
      short_sha: short(cA),
      parent_sha: mOrbit,
      base_sha: mOrbit,
      partial: false,
      message: msgA,
      created_at: ago(7 * 60),
    },
  ],
  reviews: [
    {
      id: 20,
      revision: 0,
      verdict: "approve",
      message: "Clean extraction.",
      created_at: ago(6 * 60),
    },
  ],
  diffs: {
    [diffKey(0)]: trivialDiff(
      msgA,
      "src/sched/mod.rs",
      "pub trait Scheduler {}",
    ),
  },
};

const changeD: ChangeRecord = {
  id: 52,
  repo_id: 3,
  change_key: "Idd44cc33bb22aa11",
  subject: "orbit: add a deadline clock source",
  last_reviewed_revision: null,
  revisions: [
    {
      number: 0,
      commit_sha: cD,
      short_sha: short(cD),
      parent_sha: mOrbit,
      base_sha: mOrbit,
      partial: false,
      message: msgD,
      created_at: ago(2 * 60),
    },
  ],
  reviews: [],
  diffs: {
    [diffKey(0)]: trivialDiff(msgD, "src/clock.rs", "pub struct Monotonic;"),
  },
};

// B: two patchsets. rev0 (parent A) is approved and a newer rev landed on
// main (merged_revision = 1) → its rev0 path member shows merged_elsewhere;
// rev1 (parent D) is pending. From C's chain B sits at rev0 with a newer
// patchset elsewhere (newer_elsewhere); from E's chain B sits at rev1.
const changeB: ChangeRecord = {
  id: 51,
  repo_id: 3,
  change_key: "Ibb22cc33dd44ee55",
  subject: "orbit: fair-share scheduler policy",
  last_reviewed_revision: 0,
  merged_revision: 1,
  revisions: [
    {
      number: 0,
      commit_sha: cB0,
      short_sha: short(cB0),
      parent_sha: cA,
      base_sha: mOrbit,
      partial: false,
      message: msgB,
      created_at: ago(6 * 60),
    },
    {
      number: 1,
      commit_sha: cB1,
      short_sha: short(cB1),
      parent_sha: cD,
      base_sha: mOrbit,
      partial: false,
      message: msgB,
      created_at: ago(90),
    },
  ],
  reviews: [
    {
      id: 21,
      revision: 0,
      verdict: "approve",
      message: "Weighting looks right; LGTM on this patchset.",
      created_at: ago(5 * 60),
    },
  ],
  diffs: {
    [diffKey(0)]: trivialDiff(msgB, "src/sched/fair.rs", "// fair-share v0"),
    [diffKey(1)]: trivialDiff(msgB, "src/sched/fair.rs", "// fair-share v1"),
  },
};

const changeC: ChangeRecord = {
  id: 53,
  repo_id: 3,
  change_key: "Icc33dd44ee55ff66",
  subject: "orbit: wire the fair policy into the runtime",
  last_reviewed_revision: null,
  revisions: [
    {
      number: 0,
      commit_sha: cC,
      short_sha: short(cC),
      parent_sha: cB0,
      base_sha: mOrbit,
      partial: false,
      message: msgC,
      created_at: ago(5 * 60),
    },
  ],
  reviews: [],
  diffs: {
    [diffKey(0)]: trivialDiff(
      msgC,
      "src/runtime.rs",
      "use crate::sched::fair;",
    ),
  },
};

const changeE: ChangeRecord = {
  id: 55,
  repo_id: 3,
  change_key: "Iee55ff66aa11bb22",
  subject: "orbit: deadline policy on top of fair-share",
  last_reviewed_revision: null,
  revisions: [
    {
      number: 0,
      commit_sha: cE,
      short_sha: short(cE),
      parent_sha: cB1,
      base_sha: mOrbit,
      partial: false,
      message: msgE,
      created_at: ago(80),
    },
  ],
  reviews: [],
  diffs: {
    [diffKey(0)]: trivialDiff(msgE, "src/sched/deadline.rs", "// deadline v0"),
  },
};

// ---------------------------------------------------------------------------
// The change set and the tip set (the only things the dashboard enumerates;
// every chain path is derived from parent_sha — see `walkPath`).

const changes: ChangeRecord[] = [
  change10,
  change11,
  change12,
  change20,
  change30,
  change40,
  changeA,
  changeB,
  changeC,
  changeD,
  changeE,
];

const tips: TipRecord[] = [
  // repo 1
  {
    tip_change_id: 12,
    repo_id: 1,
    revision: 0,
    name: "feat/token-rotation",
    partial: false,
    active: true,
  },
  {
    tip_change_id: 40,
    repo_id: 1,
    revision: 0,
    name: "build/rustls",
    partial: false,
    active: false, // merged — only with ?status=all
  },
  // repo 2
  {
    tip_change_id: 20,
    repo_id: 2,
    revision: 0,
    // The agent is mid-push (nit push --partial); exercises the PARTIAL badge.
    name: "fix/wal-checkpoint",
    partial: true,
    active: true,
  },
  {
    tip_change_id: 30,
    repo_id: 2,
    revision: 0,
    name: "chore/dedupe-ci-cache",
    partial: false,
    active: true,
  },
  // repo 3 — two tips through the shared change B (51)
  {
    tip_change_id: 53,
    repo_id: 3,
    revision: 0,
    name: "feat/fair-sched",
    partial: false,
    active: true,
  },
  {
    tip_change_id: 55,
    repo_id: 3,
    revision: 0,
    name: "feat/deadline-sched",
    partial: false,
    active: true,
  },
];

// ---------------------------------------------------------------------------
// Threads + drafts (published threads carry their conversation; reviewer
// drafts reply to one or open a new thread; anchors served verbatim — the
// client places them by diff range, docs/api.md "Comment placement")

const threads: ThreadRecord[] = [
  // change 10 — a change-level remark, published with the approval.
  {
    id: 70,
    change_id: 10,
    revision: 0,
    file: null,
    line: null,
    side: "new",
    line_text: null,
    resolved: true,
    comments: [
      {
        author: "reviewer",
        body: "Consider a partial index on revoked=0 if the table grows; not blocking.",
        review_id: 4,
        created_at: ago(22 * 60),
      },
    ],
    created_at: ago(22 * 60),
    updated_at: ago(22 * 60),
  },
  // change 11 — resolved thread (anchor survives the amend, line shifts).
  {
    id: 71,
    change_id: 11,
    revision: 0,
    file: "src/auth/rotate.rs",
    line: 22,
    side: "new",
    line_text: "        let fresh = Token::generate(&mut self.rng.lock());",
    resolved: true,
    comments: [
      {
        author: "reviewer",
        body:
          "Locking the RNG mutex inside rotate() serializes every refresh — " +
          "worth a thread-local RNG?",
        review_id: 5,
        created_at: ago(21 * 60),
      },
      {
        author: "agent",
        body:
          "ChaCha12 keystream behind the lock costs ~11ns per token; a " +
          "thread-local would add per-thread reseeding. Keeping the mutex.",
        review_id: null,
        created_at: ago(110),
      },
    ],
    created_at: ago(21 * 60),
    updated_at: ago(110),
  },
  // change 11 — unresolved thread on store.rs.
  {
    id: 73,
    change_id: 11,
    revision: 0,
    file: "src/auth/store.rs",
    line: 58,
    side: "new",
    line_text: "        let conn = self.pool.clone().get();",
    resolved: false,
    comments: [
      {
        author: "reviewer",
        body:
          "Why clone the pool for a second connection? lookup() and " +
          "mark_rotated() on different connections lose the transaction.",
        review_id: 5,
        created_at: ago(21 * 60),
      },
      {
        author: "agent",
        body:
          "The pool connection is held across an await in the caller; cloning " +
          "avoids a deadlock. Can wrap both calls in one connection if you " +
          "prefer — say the word.",
        review_id: null,
        created_at: ago(105),
      },
    ],
    created_at: ago(21 * 60),
    updated_at: ago(105),
  },
  // change 11 — unresolved range thread: the selection spans the
  // generate/mark pair (partial first line through mid last line) and
  // survives the amend, shifting 22-23 → 30-31 with chars intact.
  {
    id: 79,
    change_id: 11,
    revision: 0,
    file: "src/auth/rotate.rs",
    line: 23,
    side: "new",
    range: { start_line: 22, start_char: 8, end_line: 23, end_char: 50 },
    line_text: "        self.store.mark_rotated(entry.id, &fresh);",
    resolved: false,
    comments: [
      {
        author: "reviewer",
        body:
          "Generate-then-mark isn't atomic: a crash between these two " +
          "statements hands out a token the store never recorded.",
        review_id: 5,
        created_at: ago(21 * 60),
      },
    ],
    created_at: ago(21 * 60),
    updated_at: ago(21 * 60),
  },
  // change 11 — resolved thread on a line rewritten in r1; it stays pinned
  // to r0 and shows on the left of the r0 → r1 interdiff.
  {
    id: 75,
    change_id: 11,
    revision: 0,
    file: "src/auth/rotate.rs",
    line: 21,
    side: "new",
    line_text: "        let entry = self.store.lookup(presented).unwrap();",
    resolved: true,
    comments: [
      {
        author: "reviewer",
        body:
          "This unwrap is a production panic on any unknown token — return a " +
          "typed error and map it to 401 at the edge.",
        review_id: 5,
        created_at: ago(21 * 60),
      },
      {
        author: "agent",
        body:
          "Done in r2: lookup() errors are typed (RotateError) and reuse " +
          "now revokes the family.",
        review_id: null,
        created_at: ago(100),
      },
    ],
    created_at: ago(21 * 60),
    updated_at: ago(100),
  },
  // change 11 — resolved thread on the commit message: the r1 reword
  // answers it; the anchored line survives unchanged (no shift needed).
  {
    id: 77,
    change_id: 11,
    revision: 0,
    file: COMMIT_MSG_PATH,
    line: 5,
    side: "new",
    // Single-line partial selection: "the legitimate client refreshes."
    range: { start_line: 5, start_char: 7, end_line: 5, end_char: 40 },
    line_text: "moment the legitimate client refreshes.",
    resolved: true,
    comments: [
      {
        author: "reviewer",
        body:
          "The body never says what happens on token *reuse* — state the " +
          "family-revocation behavior here; it's the headline of this change.",
        review_id: 5,
        created_at: ago(21 * 60),
      },
      {
        author: "agent",
        body:
          "Reworded: the message now calls out family revocation " +
          "(RFC 6819 §5.2.2.3).",
        review_id: null,
        created_at: ago(96),
      },
    ],
    created_at: ago(21 * 60),
    updated_at: ago(96),
  },
  // change 20 — two unresolved threads from the request_changes review.
  {
    id: 80,
    change_id: 20,
    revision: 0,
    file: "src/wal.rs",
    line: 94,
    side: "new",
    line_text: "        if self.backlog_bytes() < CHECKPOINT_BACKLOG {",
    resolved: false,
    comments: [
      {
        author: "reviewer",
        body: "Hard-coded 4MiB will thrash small deployments — read it from Config.",
        review_id: 6,
        created_at: ago(3 * 60),
      },
    ],
    created_at: ago(3 * 60),
    updated_at: ago(3 * 60),
  },
  {
    id: 81,
    change_id: 20,
    revision: 0,
    file: "src/wal/backoff.rs",
    line: 3,
    side: "old",
    line_text: "pub fn jitter(base: Duration) -> Duration {",
    resolved: false,
    comments: [
      {
        author: "reviewer",
        body: "compactor.rs still calls jitter(); this won't build.",
        review_id: 6,
        created_at: ago(3 * 60),
      },
    ],
    created_at: ago(3 * 60),
    updated_at: ago(3 * 60),
  },
  // change 51 (B) — a shared thread on rev0, the patchset C's chain walks.
  // It belongs to the change, so both chains see it; it renders against rev0.
  {
    id: 82,
    change_id: 51,
    revision: 0,
    file: "src/sched/fair.rs",
    line: 2,
    side: "new",
    line_text: "// fair-share v0",
    resolved: false,
    comments: [
      {
        author: "reviewer",
        body:
          "Recent-share window: is it EWMA or a fixed ring? Spell it out — it " +
          "decides how fast a task recovers priority.",
        review_id: 21,
        created_at: ago(5 * 60),
      },
    ],
    created_at: ago(5 * 60),
    updated_at: ago(5 * 60),
  },
];

// change 11 — the reviewer's in-progress drafts on revision 1: two new
// threads on the new side, plus one on the old (red) side — a remark on the
// pre-change code, which the old column of the base → r1 diff is for
// (docs/api.md "Comment placement"). All open new threads (thread_id null).
const drafts: DraftRecord[] = [
  {
    id: 100,
    change_id: 11,
    thread_id: null,
    revision: 1,
    file: "src/auth/rotate.rs",
    line: 26,
    side: "new",
    line_text:
      "            // Reuse detected: revoke the whole family (RFC 6819 §5.2.2.3).",
    body: "Put the RFC section in the error message too — operators grep for it.",
    resolved: false,
    created_at: ago(30),
    updated_at: ago(30),
    // The anchored line does not exist in revision 0's tree.
  },
  {
    id: 101,
    change_id: 11,
    thread_id: null,
    revision: 1,
    file: "tests/rotation.rs",
    line: 13,
    side: "new",
    line_text: "    let _ = rotator.rotate(&seeded).unwrap();",
    body: "Also assert the family row is revoked — this only checks the error value.",
    resolved: false,
    created_at: ago(25),
    updated_at: ago(25),
    // tests/rotation.rs does not exist at revision 0.
  },
  {
    id: 102,
    change_id: 11,
    thread_id: null,
    revision: 1,
    file: "src/auth/rotate.rs",
    line: 20,
    side: "old",
    line_text: "    pub fn rotate(&self, presented: &str) -> Token {",
    body:
      "The old signature returned Token directly; every caller now has to " +
      "handle the Result — make sure none silently unwraps it.",
    resolved: false,
    created_at: ago(22),
    updated_at: ago(22),
  },
];

let nextDraftId = 200;
let nextThreadId = 300;
let nextReviewId = 50;

// Reviewer decision drafts (docs/api.md "Reviewer decisions"): one staged
// decision per change, mutable, published on chain batch submit — the mock of
// the server's draft_reviews side table. Seed one so the dashboard drawer's
// submit button + draft-state pill and the change-page staged chip render.
const draftReviews = new Map<number, { decision: Decision; message: string }>();
draftReviews.set(12, {
  decision: "request_changes",
  message: "Inline the sequence diagram as text — the PNG won't review.",
});

/** Drain a change's comment drafts into `review`, opening or updating their
 * threads; returns the threads it touched. Shared by the immediate POST
 * /reviews and the batch submit (docs/api.md "Thread resolution"). */
function drainComments(
  c: ChangeRecord,
  review: Review,
  now: string,
): ThreadRecord[] {
  const touched = new Map<number, ThreadRecord>();
  const changeDrafts = drafts
    .filter((x) => x.change_id === c.id)
    .sort((a, b) => a.id - b.id);
  for (const d of changeDrafts) {
    const hasBody = d.body.trim() !== "";
    if (d.thread_id !== null) {
      const thread = threads.find((x) => x.id === d.thread_id);
      if (thread) {
        thread.resolved = d.resolved;
        thread.updated_at = now;
        if (hasBody) {
          thread.comments.push({
            author: "reviewer",
            body: d.body,
            review_id: review.id,
            created_at: now,
          });
        }
        touched.set(thread.id, thread);
      }
    } else if (hasBody) {
      const thread: ThreadRecord = {
        id: nextThreadId++,
        change_id: c.id,
        revision: d.revision,
        file: d.file,
        line: d.line,
        side: d.side,
        range: d.range ?? null,
        line_text: d.line_text,
        resolved: d.resolved,
        comments: [
          {
            author: "reviewer",
            body: d.body,
            review_id: review.id,
            created_at: now,
          },
        ],
        created_at: now,
        updated_at: now,
      };
      threads.push(thread);
      touched.set(thread.id, thread);
    }
    drafts.splice(drafts.indexOf(d), 1);
  }
  return [...touched.values()];
}

/** Why a staged decision can't publish against the change's lifecycle, or null
 * (mirrors the server's decision_block). */
function decisionBlock(c: ChangeRecord, decision: Decision): string | null {
  if (c.terminal === "merged") return "change is merged — nothing to submit";
  if (c.terminal === "abandoned") {
    return decision === "reopen"
      ? null
      : "change is abandoned — stage Reopen first";
  }
  return decision === "reopen"
    ? "change is live — Reopen does not apply"
    : null;
}

/** Publish one staged decision (mirrors the server's publish_member): an
 * optional reopen, a review draining comment drafts (the decision's verdict, or
 * `comment` to carry staged comments under a lifecycle decision), then an
 * optional abandon. */
function publishMember(
  c: ChangeRecord,
  decision: Decision,
  message: string,
  revision: number,
  now: string,
): void {
  if (decision === "reopen") c.terminal = undefined;
  const hasComments = drafts.some((d) => d.change_id === c.id);
  const verdict: Verdict | null =
    decision === "approve" ||
    decision === "request_changes" ||
    decision === "comment"
      ? decision
      : hasComments
        ? "comment"
        : null;
  if (verdict) {
    const review: Review = {
      id: nextReviewId++,
      revision,
      verdict,
      message: decision === verdict ? message : "",
      created_at: now,
    };
    c.reviews.push(review);
    drainComments(c, review, now);
    c.last_reviewed_revision = Math.max(
      c.last_reviewed_revision ?? 0,
      revision,
    );
  }
  if (decision === "abandon") c.terminal ??= "abandoned";
}

// ---------------------------------------------------------------------------
// Derivations (status, counts, chain state, path) so mutations stay consistent

const WEB_BASE = "http://127.0.0.1:8877";

/** The commit-sha → (change, revision) index — the basis for the SHA-walk
 * that derives every chain path (docs/api.md "Chains"). */
const shaIndex = new Map<
  string,
  { change: ChangeRecord; revision: Revision }
>();
for (const c of changes) {
  for (const r of c.revisions)
    shaIndex.set(r.commit_sha, { change: c, revision: r });
}

const latestRevision = (c: ChangeRecord): Revision => {
  const r = c.revisions[c.revisions.length - 1];
  if (!r) throw new Error(`change ${c.id} has no revisions`);
  return r;
};

/** A change's displayed status at a given revision (docs/api.md "State
 * table"): terminal wins; else the verdict of the latest review at that
 * revision, falling back to pending. */
function statusAt(c: ChangeRecord, revision: number): ChangeStatus {
  if (c.terminal) return c.terminal;
  const review = c.reviews
    .filter((r) => r.revision === revision)
    .sort((a, b) => a.id - b.id)
    .at(-1);
  if (!review) return "pending";
  const byVerdict: Record<Verdict, ChangeStatus> = {
    approve: "approved",
    request_changes: "changes_requested",
    comment: "commented",
  };
  return byVerdict[review.verdict];
}

/** Walk a tip back to base through parent_sha, oldest-first (base → tip).
 * Each member pins the revision the tip walked through (the sha in the
 * index); the walk stops at a parent_sha that is no change (the merge-base
 * on the canonical branch). */
function walkPath(
  tip: TipRecord,
): { change: ChangeRecord; revision: Revision }[] {
  const tipChange = changes.find((c) => c.id === tip.tip_change_id);
  if (!tipChange) throw new Error(`unknown tip change ${tip.tip_change_id}`);
  const tipRev =
    tipChange.revisions.find((r) => r.number === tip.revision) ??
    latestRevision(tipChange);
  const out: { change: ChangeRecord; revision: Revision }[] = [
    { change: tipChange, revision: tipRev },
  ];
  let parent = tipRev.parent_sha;
  for (
    let member = shaIndex.get(parent);
    member !== undefined;
    member = shaIndex.get(parent)
  ) {
    out.push(member);
    parent = member.revision.parent_sha;
  }
  return out.reverse(); // base → tip
}

function pathEntry(
  member: { change: ChangeRecord; revision: Revision },
  position: number,
): PathEntry {
  const { change: c, revision: rev } = member;
  const ownThreads = threads.filter(
    (t) => t.change_id === c.id && t.revision === rev.number,
  );
  const ownDrafts = drafts.filter(
    (d) => d.change_id === c.id && d.revision === rev.number,
  );
  const latest = latestRevision(c).number;
  return {
    change_id: c.id,
    position,
    change_key: c.change_key,
    revision: rev.number,
    latest_revision: latest,
    newer_elsewhere: latest > rev.number,
    status: statusAt(c, rev.number),
    merged_elsewhere:
      c.merged_revision !== undefined && c.merged_revision > rev.number,
    subject: c.subject,
    commit_sha: rev.commit_sha,
    short_sha: rev.short_sha,
    counts: {
      threads: ownThreads.length,
      drafts: ownDrafts.length,
      unresolved: ownThreads.filter((t) => !t.resolved).length,
    },
    draft_decision: draftReviews.get(c.id)?.decision ?? null,
  };
}

function derivePath(tip: TipRecord): PathEntry[] {
  return walkPath(tip).map((m, i) => pathEntry(m, i));
}

/** A chain's derived state from its path members (docs/api.md state table).
 * Abandonment is derivation-inert: abandoned members are dropped before the
 * rollup, and there is no abandoned chain state. */
function chainState(tip: TipRecord, path: PathEntry[]): ChainState {
  const live = path.filter((e) => e.status !== "abandoned");
  if (live.length === 0) return "agents_turn"; // empty or all-abandoned tip
  if (live.every((e) => e.status === "merged")) return "merged";
  if (
    live.some(
      (e) => e.status === "changes_requested" || e.status === "commented",
    )
  ) {
    return "agents_turn";
  }
  if (live.some((e) => e.status === "pending")) return "waiting_for_review";
  // The rest are approved (≥1) and/or merged, no pending — approved, unless the
  // tip is still partial (the agent is pushing), which is agents_turn.
  return tip.partial ? "agents_turn" : "approved";
}

const newestEntryTime = (path: PathEntry[]): string => {
  // The newest member-entry time across the path; fall back to the latest
  // revision's created_at via the change set.
  let newest = "";
  for (const e of path) {
    const c = changes.find((x) => x.id === e.change_id);
    const rev = c?.revisions.find((r) => r.number === e.revision);
    for (const t of [
      rev?.created_at,
      ...threads
        .filter((th) => th.change_id === e.change_id)
        .map((th) => th.updated_at),
    ]) {
      if (t && t > newest) newest = t;
    }
  }
  return newest;
};

function chainSummary(tip: TipRecord): ChainSummary {
  const path = derivePath(tip);
  return {
    tip_change_id: tip.tip_change_id,
    repo_id: tip.repo_id,
    name: tip.name,
    state: chainState(tip, path),
    partial: tip.partial,
    web_url: `${WEB_BASE}/repos/${tip.repo_id}#chain-${tip.tip_change_id}`,
    updated_at: newestEntryTime(path),
    path,
  };
}

function chainView(tip: TipRecord): Chain {
  const path = derivePath(tip);
  const repo = repos.find((r) => r.id === tip.repo_id);
  return {
    tip_change_id: tip.tip_change_id,
    repo_id: tip.repo_id,
    name: tip.name,
    base_branch: repo?.base_branch ?? "main",
    state: chainState(tip, path),
    partial: tip.partial,
    web_url: `${WEB_BASE}/repos/${tip.repo_id}#chain-${tip.tip_change_id}`,
    path,
  };
}

/** Resolve `GET /chains/{change_id}?revision=N` to a tip (mirrors the backend's
 * `tip_for`): a live tip whose path walks `changeId` at that revision, else the
 * change as its own degenerate tip. So an INTERIOR change resolves to the tip
 * that extends through it (the full chain), not a 404. */
function resolveTip(
  changeId: number,
  revision?: number,
): TipRecord | undefined {
  const c = changes.find((x) => x.id === changeId);
  if (!c) return undefined;
  const rev = revision ?? latestRevision(c).number;
  for (const tip of tips) {
    const member = derivePath(tip).find((e) => e.change_id === changeId);
    if (member?.revision === rev) return tip;
  }
  // No live tip pins this (change, revision): the change is its own tip.
  return {
    tip_change_id: changeId,
    repo_id: c.repo_id,
    revision: rev,
    name: c.change_key.slice(0, 8),
    partial: false,
    active: !c.terminal,
  };
}

/** Every tip walking through a change, each with the patchset it pins there
 * (docs/api.md `ChainRef`). */
function chainsThrough(c: ChangeRecord): ChainRef[] {
  const refs: ChainRef[] = [];
  for (const tip of tips) {
    const member = derivePath(tip).find((e) => e.change_id === c.id);
    if (!member) continue;
    refs.push({
      tip_change_id: tip.tip_change_id,
      revision: member.revision,
      name: tip.name,
      web_url: `${WEB_BASE}/repos/${c.repo_id}#chain-${tip.tip_change_id}`,
    });
  }
  return refs;
}

/** Derive the repo registry (docs/api.md `GET /api/repos`). `active_chains`
 * is the live tip count for the repo. */
function repoList(): Repo[] {
  return repos.map((r) => ({
    id: r.id,
    git_dir: r.git_dir,
    base_branch: r.base_branch,
    active_chains: tips.filter((t) => t.repo_id === r.id && t.active).length,
  }));
}

/** A thread/draft record → its wire shape; anchors are served verbatim (the
 * client places them by diff range, docs/api.md "Comment placement"). */
function renderThread(t: ThreadRecord): Thread {
  return { ...t, range: t.range ?? null };
}
function renderDraft(d: DraftRecord): Draft {
  return { ...d, range: d.range ?? null };
}

function changeDetail(c: ChangeRecord): ChangeDetail {
  return {
    id: c.id,
    repo_id: c.repo_id,
    change_key: c.change_key,
    subject: c.subject,
    last_reviewed_revision: c.last_reviewed_revision,
    revisions: c.revisions,
    threads: threads.filter((x) => x.change_id === c.id).map(renderThread),
    drafts: drafts.filter((x) => x.change_id === c.id).map(renderDraft),
    reviews: c.reviews,
    chains: chainsThrough(c),
    draft_decision: draftReviews.get(c.id) ?? null,
  };
}

/** Find the text of a diff line so new drafts get a line_text snapshot. */
function snapshotLineText(
  c: ChangeRecord,
  revision: number,
  file: string | undefined,
  line: number | undefined,
  side: CommentSide,
): string | null {
  if (!file || line === undefined) return null;
  const diff = c.diffs[diffKey(revision)];
  const f = diff?.files.find((x) => x.path === file || x.old_path === file);
  if (!f) return null;
  for (const hunk of f.hunks) {
    for (const l of hunk.lines) {
      if (side === "new" ? l.new === line : l.old === line) return l.text;
    }
  }
  return null;
}

const notFound = (what: string): never => {
  throw new ApiError(404, `${what} not found`);
};

const getChange = (id: number): ChangeRecord =>
  changes.find((c) => c.id === id) ?? notFound(`change ${id}`);

// ---------------------------------------------------------------------------
// The mock router — mirrors the endpoint table in docs/api.md

const LATENCY_MS = 40;

export async function mockRequest(
  method: string,
  path: string,
  body?: unknown,
): Promise<unknown> {
  await new Promise((r) => setTimeout(r, LATENCY_MS));
  const url = new URL(path, "http://mock");
  const p = url.pathname;
  const q = url.searchParams;
  let m: RegExpExecArray | null;

  if (method === "GET" && p === "/repos") {
    return { repos: repoList() };
  }

  if (method === "GET" && p === "/chains") {
    const status = q.get("status") ?? "active";
    const repo = q.get("repo");
    const listed = tips.filter(
      (t) =>
        (status === "all" || t.active) &&
        (repo === null || t.repo_id === Number(repo)),
    );
    return { chains: listed.map(chainSummary) };
  }

  // The aggregated chain log is not in this cut (events return later); serve
  // an empty timeline so the endpoint exists.
  if ((m = /^\/chains\/(\d+)\/log$/.exec(p)) && method === "GET") {
    const id = Number(m[1]);
    if (!tips.some((t) => t.tip_change_id === id))
      return notFound(`chain ${id}`);
    return { entries: [] };
  }

  if ((m = /^\/chains\/(\d+)$/.exec(p)) && method === "GET") {
    const id = Number(m[1]);
    const revision = q.has("revision") ? Number(q.get("revision")) : undefined;
    const tip = resolveTip(id, revision);
    if (!tip) return notFound(`chain ${id}`);
    return chainView(tip);
  }

  // Batch submit: publish every chain member's staged decision at the revision
  // the path pins, each independently (docs/api.md "Chains").
  if ((m = /^\/chains\/(\d+)\/submit$/.exec(p)) && method === "POST") {
    const id = Number(m[1]);
    const revision = q.has("revision") ? Number(q.get("revision")) : undefined;
    const tip = resolveTip(id, revision);
    if (!tip) return notFound(`chain ${id}`);
    const now = new Date().toISOString();
    let submitted = 0;
    const errors: { change_id: number; message: string }[] = [];
    for (const member of derivePath(tip)) {
      const staged = draftReviews.get(member.change_id);
      if (!staged) continue; // no decision — leave the member's comment drafts
      const c = changes.find((x) => x.id === member.change_id);
      if (!c) continue;
      const block = decisionBlock(c, staged.decision);
      if (block) {
        errors.push({ change_id: c.id, message: block });
        continue;
      }
      publishMember(c, staged.decision, staged.message, member.revision, now);
      draftReviews.delete(c.id);
      submitted++;
    }
    return { submitted, errors };
  }

  if ((m = /^\/changes\/(\d+)$/.exec(p)) && method === "GET") {
    return changeDetail(getChange(Number(m[1])));
  }

  if (
    (m = /^\/changes\/(\d+)\/revisions\/(\d+)\/diff$/.exec(p)) &&
    method === "GET"
  ) {
    const c = getChange(Number(m[1]));
    const revision = Number(m[2]);
    const against = q.has("against") ? Number(q.get("against")) : undefined;
    const rev = c.revisions.find((r) => r.number === revision);
    if (!rev) notFound(`revision ${revision}`);
    const diff = c.diffs[diffKey(revision, against)];
    if (!diff) notFound(`diff for revision ${revision}`);
    return structuredClone(diff);
  }

  if ((m = /^\/changes\/(\d+)\/drafts$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    const req = body as CreateDraftRequest;
    const side: CommentSide = req.side ?? "new";
    const now = new Date().toISOString();
    const record: DraftRecord = {
      id: nextDraftId++,
      change_id: c.id,
      thread_id: req.thread_id ?? null,
      revision: req.revision,
      file: req.file ?? null,
      line: req.line ?? null,
      side,
      range: req.range ?? null,
      line_text: snapshotLineText(c, req.revision, req.file, req.line, side),
      body: req.body,
      resolved: req.resolved ?? false,
      created_at: now,
      updated_at: now,
    };
    drafts.push(record);
    return renderDraft(record);
  }

  if ((m = /^\/drafts\/(\d+)$/.exec(p)) && method === "PATCH") {
    const id = Number(m[1]);
    const d = drafts.find((x) => x.id === id);
    if (!d) return notFound(`draft ${id}`);
    const req = body as { body: string; resolved?: boolean };
    d.body = req.body;
    if (req.resolved !== undefined) d.resolved = req.resolved;
    d.updated_at = new Date().toISOString();
    return renderDraft(d);
  }

  if ((m = /^\/drafts\/(\d+)$/.exec(p)) && method === "DELETE") {
    const id = Number(m[1]);
    const i = drafts.findIndex((x) => x.id === id);
    if (i < 0) notFound(`draft ${id}`);
    drafts.splice(i, 1);
    return undefined;
  }

  if ((m = /^\/changes\/(\d+)\/reviews$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    const req = body as SubmitReviewRequest;
    const latest = latestRevision(c).number;
    if (req.revision !== latest) {
      // The pure-rebase auto-retarget path can't occur in fixtures; any
      // stale revision is a real conflict here.
      throw new ApiError(
        409,
        `revision ${req.revision} is no longer latest (now ${latest})`,
      );
    }
    const now = new Date().toISOString();
    const review: Review = {
      id: nextReviewId++,
      revision: req.revision,
      verdict: req.verdict,
      message: req.message,
      created_at: now,
    };
    c.reviews.push(review);
    const touched = drainComments(c, review, now);
    c.last_reviewed_revision = Math.max(
      c.last_reviewed_revision ?? 0,
      req.revision,
    );
    draftReviews.delete(c.id); // an immediate review supersedes any staged decision
    return { review, threads: touched.map(renderThread) };
  }

  // Stage / clear a reviewer decision (drafted like a comment; published by the
  // chain batch submit above) — docs/api.md "Reviewer decisions".
  if ((m = /^\/changes\/(\d+)\/decision$/.exec(p)) && method === "PUT") {
    const c = getChange(Number(m[1]));
    const req = body as StageDecisionRequest;
    const staged = { decision: req.decision, message: req.message };
    draftReviews.set(c.id, staged);
    return staged;
  }

  if ((m = /^\/changes\/(\d+)\/decision$/.exec(p)) && method === "DELETE") {
    const c = getChange(Number(m[1]));
    draftReviews.delete(c.id);
    return undefined;
  }

  if ((m = /^\/changes\/(\d+)\/abandon$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    c.terminal ??= "abandoned";
    return changeDetail(c);
  }

  if ((m = /^\/changes\/(\d+)\/reopen$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    if (c.terminal === "abandoned") c.terminal = undefined;
    return changeDetail(c);
  }

  throw new ApiError(404, `mock: no route for ${method} ${path}`);
}
