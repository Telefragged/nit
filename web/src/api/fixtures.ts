// Contract-true canned data + a tiny in-memory implementation of the API
// from docs/api.md. client.ts routes every call here when VITE_MOCK is set,
// so the whole UI (including drafts, resolve, review submission and 409s)
// works without a backend. The data doubles as component-test fixtures.
//
// Coverage on purpose:
//   chain 1  waiting_for_review — 3 changes; change 11 has 2 revisions
//            (amended in place, interdiff available), a resolved thread,
//            an unresolved thread, a thread on a line r2 rewrote (all
//            pinned to r1, so they land on the left of the r1 → r2
//            interdiff), 2 drafts, plus a resolved thread on its commit
//            message (/COMMIT_MSG) and a reworded r2 message so the
//            interdiff carries a real message diff; change 12's diff has a
//            rename and a binary file.
//   chain 2  agents_turn — a changes_requested change, mid-push (partial),
//            plus a Change-Id-validation scan error.
//   chain 3  approved — single approved change.
//   chain 4  merged — only visible via ?status=all.
//
// Every stored diff leads with the synthetic /COMMIT_MSG file, like the
// real server (docs/api.md "The commit message as a file").

import { ApiError } from "./client";
import { COMMIT_MSG_PATH } from "./types";
import type {
  Chain,
  ChainState,
  ChainStatus,
  ChangeDetail,
  ChangeStatus,
  ChangeSummary,
  Comment,
  CommentAuthor,
  CommentRange,
  CommentSide,
  CommentState,
  CreateDraftRequest,
  Diff,
  DiffFile,
  Line,
  Review,
  Revision,
  SubmitReviewRequest,
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

interface ChainRecord {
  id: number;
  repo_path: string;
  branch: string;
  base: string;
  status: ChainStatus;
  /** Sticky; set by push --partial, cleared by ready. */
  partial: boolean;
  last_scan_error: string | null;
  created_at: string;
  updated_at: string;
  /** Change ids in chain order. */
  change_ids: number[];
}

interface ChangeRecord {
  id: number;
  chain_id: number;
  change_key: string;
  position: number | null;
  status: ChangeStatus;
  subject: string;
  last_reviewed_revision: number | null;
  revisions: Revision[];
  reviews: Review[];
  /** Keyed by diffKey(revision, against). */
  diffs: Record<string, Diff>;
}

interface CommentRecord {
  id: number;
  change_id: number;
  revision: number;
  parent_id: number | null;
  author: CommentAuthor;
  file: string | null;
  line: number | null;
  side: CommentSide;
  /** Selected-text anchor; most fixture comments are whole-line. */
  range?: CommentRange | null;
  line_text: string | null;
  body: string;
  state: CommentState;
  resolved: boolean;
  review_id: number | null;
  created_at: string;
  updated_at: string;
}

const diffKey = (revision: number, against?: number) =>
  against === undefined ? `r${revision}` : `r${against}..r${revision}`;

// ---------------------------------------------------------------------------
// Chain 1 — feat/token-rotation (waiting_for_review)

const c10r1 = sha(101);
const c11r1 = sha(111);
const c11r2 = sha(112);
const c12r1 = sha(121);
const parent10 = sha(100);

const msg10r1 =
  "auth: add TokenStore schema and config plumbing\n\n" +
  "Refresh tokens get their own table keyed by token hash, with a\n" +
  "family id so a later change can revoke descendants in one\n" +
  "statement. Config grows [auth.rotation] with a ttl knob.\n\n" +
  "Change-Id: I9a41c7e2b3d4f5a6";

const change10: ChangeRecord = {
  id: 10,
  chain_id: 1,
  change_key: "I9a41c7e2b3d4f5a6",
  position: 0,
  status: "approved",
  subject: "auth: add TokenStore schema and config plumbing",
  last_reviewed_revision: 1,
  revisions: [
    {
      number: 1,
      commit_sha: c10r1,
      short_sha: short(c10r1),
      parent_sha: parent10,
      message: msg10r1,
      created_at: ago(26 * 60),
    },
  ],
  reviews: [
    {
      id: 4,
      revision: 1,
      verdict: "approve",
      message:
        "Schema is right, hash-keyed lookup avoids the timing leak. LGTM.",
      created_at: ago(22 * 60),
    },
  ],
  diffs: {
    [diffKey(1)]: {
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
// r2 rewords the message (answering the /COMMIT_MSG thread below), so
// the r1 → r2 interdiff carries a real message diff.
const msg11r2 =
  "auth: rotate refresh tokens on use\n\n" +
  "Every presented refresh token is exchanged for a fresh one and the\n" +
  "old row is marked rotated, so a stolen token stops working the\n" +
  "moment the legitimate client refreshes.\n\n" +
  "Token reuse now revokes the whole family (RFC 6819 §5.2.2.3).\n\n" +
  "Change-Id: I3f2d8a91c0b7e514";

const change11: ChangeRecord = {
  id: 11,
  chain_id: 1,
  change_key: "I3f2d8a91c0b7e514",
  position: 1,
  status: "pending",
  subject: "auth: rotate refresh tokens on use",
  last_reviewed_revision: 1,
  revisions: [
    {
      number: 1,
      commit_sha: c11r1,
      short_sha: short(c11r1),
      parent_sha: c10r1,
      message: msg11r1,
      created_at: ago(25 * 60),
    },
    // r2 is the commit amended in place: same Change-Id, same parent,
    // new sha.
    {
      number: 2,
      commit_sha: c11r2,
      short_sha: short(c11r2),
      parent_sha: c10r1,
      message: msg11r2,
      created_at: ago(95),
    },
  ],
  reviews: [
    {
      id: 5,
      revision: 1,
      verdict: "request_changes",
      message:
        "Rotation flow is right, but the unwrap is a production panic and " +
        "token reuse has to revoke the whole family. Two threads inline.",
      created_at: ago(21 * 60),
    },
  ],
  diffs: {
    // Full diff of revision 1 (parent -> rev1 tree).
    [diffKey(1)]: {
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
    // Full diff of revision 2 (parent -> rev2 tree).
    [diffKey(2)]: {
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
    [diffKey(2, 1)]: {
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
  chain_id: 1,
  change_key: "I77b0e4f5a8123c9d",
  position: 2,
  status: "pending",
  subject: "auth: document rotation and ship flow diagram",
  last_reviewed_revision: null,
  revisions: [
    {
      number: 1,
      commit_sha: c12r1,
      short_sha: short(c12r1),
      parent_sha: c11r2,
      message: msg12r1,
      created_at: ago(90),
    },
  ],
  reviews: [],
  diffs: {
    [diffKey(1)]: {
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
// Chain 2 — fix/wal-checkpoint (agents_turn)

const c20r1 = sha(201);
const parent20 = sha(200);

const msg20r1 =
  "wal: checkpoint on idle, not on every commit\n\n" +
  "Checkpointing after each commit stalls writers; move it to the idle\n" +
  "loop with a 4MiB backlog threshold.\n\n" +
  "Change-Id: Ib8d3e6f1a4c75290";

const change20: ChangeRecord = {
  id: 20,
  chain_id: 2,
  change_key: "Ib8d3e6f1a4c75290",
  position: 0,
  status: "changes_requested",
  subject: "wal: checkpoint on idle, not on every commit",
  last_reviewed_revision: 1,
  revisions: [
    {
      number: 1,
      commit_sha: c20r1,
      short_sha: short(c20r1),
      parent_sha: parent20,
      message: msg20r1,
      created_at: ago(8 * 60),
    },
  ],
  reviews: [
    {
      id: 6,
      revision: 1,
      verdict: "request_changes",
      message:
        "Threshold needs to be configurable and the deleted backoff still " +
        "had one caller.",
      created_at: ago(3 * 60),
    },
  ],
  diffs: {
    [diffKey(1)]: {
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
// Chain 3 — chore/dedupe-ci-cache (approved)

const c30r1 = sha(301);

const msg30r1 =
  "ci: key caches on lockfile hash only\n\n" +
  "Keying on the branch name made every PR start cold.\n\n" +
  "Change-Id: Ie1f4a7b2c5d80936";

const change30: ChangeRecord = {
  id: 30,
  chain_id: 3,
  change_key: "Ie1f4a7b2c5d80936",
  position: 0,
  status: "approved",
  subject: "ci: key caches on lockfile hash only",
  last_reviewed_revision: 1,
  revisions: [
    {
      number: 1,
      commit_sha: c30r1,
      short_sha: short(c30r1),
      parent_sha: sha(300),
      message: msg30r1,
      created_at: ago(50 * 60),
    },
  ],
  reviews: [
    {
      id: 7,
      revision: 1,
      verdict: "approve",
      message: "Nice catch.",
      created_at: ago(40 * 60),
    },
  ],
  diffs: {
    [diffKey(1)]: {
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
// Chain 4 — merged (only listed with ?status=all)

const c40r1 = sha(401);

const msg40r1 =
  "build: drop unused openssl feature\n\nChange-Id: I0d9c8b7a6f5e4321";

const change40: ChangeRecord = {
  id: 40,
  chain_id: 4,
  change_key: "I0d9c8b7a6f5e4321",
  position: 0,
  status: "approved",
  subject: "build: drop unused openssl feature",
  last_reviewed_revision: 1,
  revisions: [
    {
      number: 1,
      commit_sha: c40r1,
      short_sha: short(c40r1),
      parent_sha: sha(400),
      message: msg40r1,
      created_at: ago(4 * 24 * 60),
    },
  ],
  reviews: [
    {
      id: 8,
      revision: 1,
      verdict: "approve",
      message: "",
      created_at: ago(3 * 24 * 60),
    },
  ],
  diffs: {
    [diffKey(1)]: {
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
// Chains

const chains: ChainRecord[] = [
  {
    id: 1,
    repo_path: "/home/vetle/src/acme-runtime",
    branch: "feat/token-rotation",
    base: "main",
    status: "active",
    partial: false,
    last_scan_error: null,
    created_at: ago(26 * 60),
    updated_at: ago(85),
    change_ids: [10, 11, 12],
  },
  {
    id: 2,
    repo_path: "/home/vetle/src/quarry",
    branch: "fix/wal-checkpoint",
    base: "main",
    status: "active",
    // The agent is mid-push (nit push --partial); exercises the PARTIAL badge.
    partial: true,
    // The latest push failed Change-Id validation; exercises the scan-error
    // banner (prior state stays served).
    last_scan_error:
      "commits without a Change-Id trailer (9f3c21a4d2e1) — every commit needs one",
    created_at: ago(9 * 60),
    updated_at: ago(112),
    change_ids: [20],
  },
  {
    id: 3,
    repo_path: "/home/vetle/src/quarry",
    branch: "chore/dedupe-ci-cache",
    base: "main",
    status: "active",
    partial: false,
    last_scan_error: null,
    created_at: ago(50 * 60),
    updated_at: ago(40 * 60),
    change_ids: [30],
  },
  {
    id: 4,
    repo_path: "/home/vetle/src/acme-runtime",
    branch: "build/rustls",
    base: "main",
    status: "merged",
    partial: false,
    last_scan_error: null,
    created_at: ago(5 * 24 * 60),
    updated_at: ago(3 * 24 * 60),
    change_ids: [40],
  },
];

const changes: ChangeRecord[] = [
  change10,
  change11,
  change12,
  change20,
  change30,
  change40,
];

// ---------------------------------------------------------------------------
// Comments (drafts + published; anchors served verbatim — the client
// places them by diff range, docs/api.md "Comment placement")

const comments: CommentRecord[] = [
  // change 10 — a change-level remark, published with the approval.
  {
    id: 70,
    change_id: 10,
    revision: 1,
    parent_id: null,
    author: "reviewer",
    file: null,
    line: null,
    side: "new",
    line_text: null,
    body: "Consider a partial index on revoked=0 if the table grows; not blocking.",
    state: "published",
    resolved: true,
    review_id: 4,
    created_at: ago(22 * 60),
    updated_at: ago(22 * 60),
  },
  // change 11 — resolved thread (anchor survives the amend, line shifts).
  {
    id: 71,
    change_id: 11,
    revision: 1,
    parent_id: null,
    author: "reviewer",
    file: "src/auth/rotate.rs",
    line: 22,
    side: "new",
    line_text: "        let fresh = Token::generate(&mut self.rng.lock());",
    body:
      "Locking the RNG mutex inside rotate() serializes every refresh — " +
      "worth a thread-local RNG?",
    state: "published",
    resolved: true,
    review_id: 5,
    created_at: ago(21 * 60),
    updated_at: ago(21 * 60),
  },
  {
    id: 72,
    change_id: 11,
    revision: 1,
    parent_id: 71,
    author: "agent",
    file: "src/auth/rotate.rs",
    line: 22,
    side: "new",
    line_text: "        let fresh = Token::generate(&mut self.rng.lock());",
    body:
      "ChaCha12 keystream behind the lock costs ~11ns per token; a " +
      "thread-local would add per-thread reseeding. Keeping the mutex.",
    state: "published",
    resolved: true,
    review_id: null,
    created_at: ago(110),
    updated_at: ago(110),
  },
  // change 11 — unresolved thread on store.rs.
  {
    id: 73,
    change_id: 11,
    revision: 1,
    parent_id: null,
    author: "reviewer",
    file: "src/auth/store.rs",
    line: 58,
    side: "new",
    line_text: "        let conn = self.pool.clone().get();",
    body:
      "Why clone the pool for a second connection? lookup() and " +
      "mark_rotated() on different connections lose the transaction.",
    state: "published",
    resolved: false,
    review_id: 5,
    created_at: ago(21 * 60),
    updated_at: ago(21 * 60),
  },
  {
    id: 74,
    change_id: 11,
    revision: 1,
    parent_id: 73,
    author: "agent",
    file: "src/auth/store.rs",
    line: 58,
    side: "new",
    line_text: "        let conn = self.pool.clone().get();",
    body:
      "The pool connection is held across an await in the caller; cloning " +
      "avoids a deadlock. Can wrap both calls in one connection if you " +
      "prefer — say the word.",
    state: "published",
    resolved: false,
    review_id: null,
    created_at: ago(105),
    updated_at: ago(105),
  },
  // change 11 — unresolved range thread: the selection spans the
  // generate/mark pair (partial first line through mid last line) and
  // survives the amend, shifting 22-23 → 30-31 with chars intact.
  {
    id: 79,
    change_id: 11,
    revision: 1,
    parent_id: null,
    author: "reviewer",
    file: "src/auth/rotate.rs",
    line: 23,
    side: "new",
    range: { start_line: 22, start_char: 8, end_line: 23, end_char: 50 },
    line_text: "        self.store.mark_rotated(entry.id, &fresh);",
    body:
      "Generate-then-mark isn't atomic: a crash between these two " +
      "statements hands out a token the store never recorded.",
    state: "published",
    resolved: false,
    review_id: 5,
    created_at: ago(21 * 60),
    updated_at: ago(21 * 60),
  },
  // change 11 — resolved thread on a line rewritten in r2; it stays pinned
  // to r1 and shows on the left of the r1 → r2 interdiff.
  {
    id: 75,
    change_id: 11,
    revision: 1,
    parent_id: null,
    author: "reviewer",
    file: "src/auth/rotate.rs",
    line: 21,
    side: "new",
    line_text: "        let entry = self.store.lookup(presented).unwrap();",
    body:
      "This unwrap is a production panic on any unknown token — return a " +
      "typed error and map it to 401 at the edge.",
    state: "published",
    resolved: true,
    review_id: 5,
    created_at: ago(21 * 60),
    updated_at: ago(21 * 60),
  },
  {
    id: 76,
    change_id: 11,
    revision: 1,
    parent_id: 75,
    author: "agent",
    file: "src/auth/rotate.rs",
    line: 21,
    side: "new",
    line_text: "        let entry = self.store.lookup(presented).unwrap();",
    body:
      "Done in r2: lookup() errors are typed (RotateError) and reuse " +
      "now revokes the family.",
    state: "published",
    resolved: true,
    review_id: null,
    created_at: ago(100),
    updated_at: ago(100),
  },
  // change 11 — resolved thread on the commit message: the r2 reword
  // answers it; the anchored line survives unchanged (no shift needed).
  {
    id: 77,
    change_id: 11,
    revision: 1,
    parent_id: null,
    author: "reviewer",
    file: COMMIT_MSG_PATH,
    line: 5,
    side: "new",
    // Single-line partial selection: "the legitimate client refreshes."
    range: { start_line: 5, start_char: 7, end_line: 5, end_char: 40 },
    line_text: "moment the legitimate client refreshes.",
    body:
      "The body never says what happens on token *reuse* — state the " +
      "family-revocation behavior here; it's the headline of this change.",
    state: "published",
    resolved: true,
    review_id: 5,
    created_at: ago(21 * 60),
    updated_at: ago(21 * 60),
  },
  {
    id: 78,
    change_id: 11,
    revision: 1,
    parent_id: 77,
    author: "agent",
    file: COMMIT_MSG_PATH,
    line: 5,
    side: "new",
    // Replies copy the root's whole anchor, range included.
    range: { start_line: 5, start_char: 7, end_line: 5, end_char: 40 },
    line_text: "moment the legitimate client refreshes.",
    body:
      "Reworded: the message now calls out family revocation " +
      "(RFC 6819 §5.2.2.3).",
    state: "published",
    resolved: true,
    review_id: null,
    created_at: ago(96),
    updated_at: ago(96),
  },
  // change 11 — drafts on revision 2: two on the new side, plus one on the
  // old (red) side — a remark on the pre-change code, which the old column
  // of the base → r2 diff is for (docs/api.md "Comment placement").
  {
    id: 100,
    change_id: 11,
    revision: 2,
    parent_id: null,
    author: "reviewer",
    file: "src/auth/rotate.rs",
    line: 26,
    side: "new",
    line_text:
      "            // Reuse detected: revoke the whole family (RFC 6819 §5.2.2.3).",
    body: "Put the RFC section in the error message too — operators grep for it.",
    state: "draft",
    resolved: false,
    review_id: null,
    created_at: ago(30),
    updated_at: ago(30),
    // The anchored line does not exist in revision 1's tree.
  },
  {
    id: 101,
    change_id: 11,
    revision: 2,
    parent_id: null,
    author: "reviewer",
    file: "tests/rotation.rs",
    line: 13,
    side: "new",
    line_text: "    let _ = rotator.rotate(&seeded).unwrap();",
    body: "Also assert the family row is revoked — this only checks the error value.",
    state: "draft",
    resolved: false,
    review_id: null,
    created_at: ago(25),
    updated_at: ago(25),
    // tests/rotation.rs does not exist at revision 1.
  },
  {
    id: 102,
    change_id: 11,
    revision: 2,
    parent_id: null,
    author: "reviewer",
    file: "src/auth/rotate.rs",
    line: 20,
    side: "old",
    line_text: "    pub fn rotate(&self, presented: &str) -> Token {",
    body:
      "The old signature returned Token directly; every caller now has to " +
      "handle the Result — make sure none silently unwraps it.",
    state: "draft",
    resolved: false,
    review_id: null,
    created_at: ago(22),
    updated_at: ago(22),
  },
  // change 20 — two unresolved threads from the request_changes review.
  {
    id: 80,
    change_id: 20,
    revision: 1,
    parent_id: null,
    author: "reviewer",
    file: "src/wal.rs",
    line: 94,
    side: "new",
    line_text: "        if self.backlog_bytes() < CHECKPOINT_BACKLOG {",
    body: "Hard-coded 4MiB will thrash small deployments — read it from Config.",
    state: "published",
    resolved: false,
    review_id: 6,
    created_at: ago(3 * 60),
    updated_at: ago(3 * 60),
  },
  {
    id: 81,
    change_id: 20,
    revision: 1,
    parent_id: null,
    author: "reviewer",
    file: "src/wal/backoff.rs",
    line: 3,
    side: "old",
    line_text: "pub fn jitter(base: Duration) -> Duration {",
    body: "compactor.rs still calls jitter(); this won't build.",
    state: "published",
    resolved: false,
    review_id: 6,
    created_at: ago(3 * 60),
    updated_at: ago(3 * 60),
  },
];

let nextCommentId = 200;
let nextReviewId = 50;

// ---------------------------------------------------------------------------
// Derivations (counts, chain state) so mutations stay consistent

const WEB_BASE = "http://127.0.0.1:8877";

function chainState(chain: ChainRecord): ChainState {
  if (chain.status === "merged") return "merged";
  if (chain.status === "abandoned") return "abandoned";
  const live = changes.filter(
    (c) => c.chain_id === chain.id && c.status !== "orphaned",
  );
  if (
    live.some(
      (c) => c.status === "changes_requested" || c.status === "commented",
    )
  ) {
    return "agents_turn";
  }
  if (live.some((c) => c.status === "pending")) return "waiting_for_review";
  if (live.length > 0 && live.every((c) => c.status === "approved")) {
    // All approved while partial is agents_turn, never approved — the
    // agent is still pushing (api.md state table).
    return chain.partial ? "agents_turn" : "approved";
  }
  return "agents_turn"; // empty chain
}

function changeSummary(c: ChangeRecord): ChangeSummary {
  const own = comments.filter((x) => x.change_id === c.id);
  const latest = c.revisions[c.revisions.length - 1];
  if (!latest) throw new Error(`change ${c.id} has no revisions`);
  return {
    id: c.id,
    position: c.position,
    change_key: c.change_key,
    subject: c.subject,
    status: c.status,
    revision: latest.number,
    last_reviewed_revision: c.last_reviewed_revision,
    commit_sha: latest.commit_sha,
    short_sha: latest.short_sha,
    counts: {
      revisions: c.revisions.length,
      published_comments: own.filter((x) => x.state === "published").length,
      drafts: own.filter((x) => x.state === "draft").length,
      unresolved: own.filter(
        (x) => x.state === "published" && x.parent_id === null && !x.resolved,
      ).length,
    },
  };
}

function chainView(chain: ChainRecord): Chain {
  return {
    id: chain.id,
    repo_path: chain.repo_path,
    branch: chain.branch,
    base: chain.base,
    status: chain.status,
    state: chainState(chain),
    partial: chain.partial,
    last_scan_error: chain.last_scan_error,
    web_url: `${WEB_BASE}/chains/${chain.id}`,
    created_at: chain.created_at,
    updated_at: chain.updated_at,
    changes: chain.change_ids
      .map((id) => {
        const c = changes.find((x) => x.id === id);
        if (!c) throw new Error(`unknown change ${id}`);
        return c;
      })
      .map(changeSummary),
  };
}

/** A comment record → its wire shape; anchors are served verbatim (the
 * client places them by diff range, docs/api.md "Comment placement"). */
function renderComment(c: CommentRecord): Comment {
  return { ...c, range: c.range ?? null };
}

function changeDetail(c: ChangeRecord): ChangeDetail {
  return {
    id: c.id,
    chain_id: c.chain_id,
    change_key: c.change_key,
    position: c.position,
    status: c.status,
    subject: c.subject,
    last_reviewed_revision: c.last_reviewed_revision,
    revisions: c.revisions,
    comments: comments.filter((x) => x.change_id === c.id).map(renderComment),
    reviews: c.reviews,
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

  if (method === "GET" && p === "/health") {
    return { status: "ok", version: "0.1.0-mock" };
  }

  if (method === "GET" && p === "/chains") {
    const status = q.get("status") ?? "active";
    const listed = chains.filter(
      (c) => status === "all" || c.status === "active",
    );
    return { chains: listed.map(chainView) };
  }

  if ((m = /^\/chains\/(\d+)$/.exec(p)) && method === "GET") {
    const id = Number(m[1]);
    const chain = chains.find((c) => c.id === id);
    if (!chain) return notFound(`chain ${m[1] ?? ""}`);
    return chainView(chain);
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
    const record: CommentRecord = {
      id: nextCommentId++,
      change_id: c.id,
      revision: req.revision,
      parent_id: req.parent_id ?? null,
      author: "reviewer",
      file: req.file ?? null,
      line: req.line ?? null,
      side,
      range: req.range ?? null,
      line_text: snapshotLineText(c, req.revision, req.file, req.line, side),
      body: req.body,
      state: "draft",
      resolved: req.resolved ?? false,
      review_id: null,
      created_at: now,
      updated_at: now,
    };
    comments.push(record);
    return renderComment(record);
  }

  if ((m = /^\/drafts\/(\d+)$/.exec(p)) && method === "PATCH") {
    const id = Number(m[1]);
    const c = comments.find((x) => x.id === id && x.state === "draft");
    if (!c) return notFound(`draft ${m[1] ?? ""}`);
    const req = body as { body: string; resolved?: boolean };
    c.body = req.body;
    if (req.resolved !== undefined) c.resolved = req.resolved;
    c.updated_at = new Date().toISOString();
    return renderComment(c);
  }

  if ((m = /^\/drafts\/(\d+)$/.exec(p)) && method === "DELETE") {
    const id = Number(m[1]);
    const i = comments.findIndex((x) => x.id === id && x.state === "draft");
    if (i < 0) notFound(`draft ${m[1] ?? ""}`);
    comments.splice(i, 1);
    return undefined;
  }

  if ((m = /^\/changes\/(\d+)\/reviews$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    const req = body as SubmitReviewRequest;
    const latestRev = c.revisions[c.revisions.length - 1];
    if (!latestRev) throw new Error(`change ${c.id} has no revisions`);
    const latest = latestRev.number;
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
    const published: Comment[] = [];
    // Drain drafts in creation order, applying each staged resolution to its
    // thread root (last wins); an empty-body draft resolves without becoming
    // a comment (docs/api.md "Thread resolution").
    const drafts = comments
      .filter((x) => x.change_id === c.id && x.state === "draft")
      .sort((a, b) => a.id - b.id);
    for (const d of drafts) {
      // Apply the staged resolution to the thread root (read before the reply
      // reset below) — last draft in creation order wins.
      const root = comments.find((x) => x.id === (d.parent_id ?? d.id));
      if (root) {
        root.resolved = d.resolved;
        root.updated_at = now;
      }
      if (d.body.trim() === "") {
        comments.splice(comments.indexOf(d), 1);
        continue;
      }
      d.state = "published";
      d.review_id = review.id;
      d.updated_at = now;
      // A reply's own resolved is false; the thread's state lives on its root.
      if (d.parent_id !== null) d.resolved = false;
      published.push(renderComment(d));
    }
    const statusByVerdict: Record<Verdict, ChangeStatus> = {
      approve: "approved",
      request_changes: "changes_requested",
      comment: "commented",
    };
    c.status = statusByVerdict[req.verdict];
    c.last_reviewed_revision = Math.max(
      c.last_reviewed_revision ?? 0,
      req.revision,
    );
    const chain = chains.find((x) => x.id === c.chain_id);
    if (chain) chain.updated_at = now;
    return { review, published_comments: published };
  }

  throw new ApiError(404, `mock: no route for ${method} ${path}`);
}
