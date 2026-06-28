import { describe, expect, it } from "vitest";

import { changeDetail, foldEntry, replayProj } from "./fold";
import type { LogEntry } from "./types";

const revision: LogEntry = {
  change_id: 1,
  idx: 0,
  seq: 0,
  created_at: "t0",
  kind: "revision",
  payload: {
    commit_sha: "A",
    parent_sha: "base",
    base_sha: "base",
    message: "subject\n\nChange-Id: I1\n",
    resets_status: true,
  },
};
const review: LogEntry = {
  change_id: 1,
  idx: 1,
  seq: 1,
  created_at: "t1",
  kind: "review",
  payload: {
    review_id: 5,
    revision: 0,
    verdict: "approve",
    message: "lgtm",
    comments: [],
  },
};

describe("the shared wasm fold", () => {
  it("folds a log into a ChangeProj snapshot, then projects ChangeDetail", () => {
    const proj = replayProj({
      id: 1,
      repo_id: 1,
      change_key: "I1",
      entries: [revision],
    });
    // Only the revision is folded: the high-water mark is the next idx.
    expect(proj.entries_folded).toBe(1);

    const detail = changeDetail(proj);
    expect(detail.id).toBe(1);
    expect(detail.revisions).toHaveLength(1);
    // Revision numbers are minted in the fold, 0-based.
    expect(detail.revisions[0]?.number).toBe(0);
    expect(detail.reviews).toHaveLength(0);
    // Drafts/decision are not log state.
    expect(detail.drafts).toEqual([]);
    expect(detail.draft_decision).toBeNull();
  });

  it("folds the live tail onto the snapshot, idempotent across the overlap", () => {
    const snapshot = replayProj({
      id: 1,
      repo_id: 1,
      change_key: "I1",
      entries: [revision],
    });

    const advanced = foldEntry(snapshot, review);
    expect(advanced.entries_folded).toBe(2);
    expect(changeDetail(advanced).reviews).toHaveLength(1);
    expect(changeDetail(advanced).reviews[0]?.verdict).toBe("approve");

    // Re-delivering an entry the snapshot already covered is a no-op.
    const replayed = foldEntry(advanced, review);
    expect(replayed.entries_folded).toBe(2);
    expect(changeDetail(replayed).reviews).toHaveLength(1);
  });
});
