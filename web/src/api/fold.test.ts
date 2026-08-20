import { describe, expect, it } from "vitest";

import { changeDetail, foldEntry, replayProjection } from "./fold";
import type { LogEntry } from "./types";
import { shaOf as sha } from "./fixtures/builders";

const revision: LogEntry = {
  change_number: 1,
  position: 0,
  sequence: 0,
  created_at: "t0",
  kind: "revision",
  payload: {
    commit_sha: sha("A"),
    parent_sha: sha("base"),
    fork_sha: sha("base"),
    message: "subject\n\nChange-Id: I1\n",
    resets_status: true,
  },
};
const review: LogEntry = {
  change_number: 1,
  position: 1,
  sequence: 1,
  created_at: "t1",
  kind: "review",
  payload: {
    revision: 0,
    verdict: "approve",
    message: "lgtm",
    comments: [],
  },
};

describe("the shared wasm fold", () => {
  it("folds a log into a ChangeProjection, then projects ChangeDetail", () => {
    const proj = replayProjection({
      id: 1,
      repo_id: 1,
      change_id: "I1",
      entries: [revision],
    });
    // entries_folded is the high-water mark (next position to fold), not a count.
    expect(proj.entries_folded).toBe(1);

    const detail = changeDetail(proj);
    expect(detail.id).toBe(1);
    expect(detail.revisions).toHaveLength(1);
    // Revision numbers are minted in the fold, not carried in the entry.
    expect(detail.revisions[0]?.number).toBe(0);
    expect(detail.reviews).toHaveLength(0);
    // Drafts/decision are not log state.
    expect(detail.drafts).toEqual([]);
    expect(detail.draft_decision).toBeNull();
  });

  it("folds the live tail onto the projection, idempotent across the overlap", () => {
    const projection = replayProjection({
      id: 1,
      repo_id: 1,
      change_id: "I1",
      entries: [revision],
    });

    const advanced = foldEntry(projection, review);
    expect(advanced.entries_folded).toBe(2);
    expect(changeDetail(advanced).reviews).toHaveLength(1);
    expect(changeDetail(advanced).reviews[0]?.verdict).toBe("approve");

    // Re-delivering an entry the projection already covered is a no-op.
    const replayed = foldEntry(advanced, review);
    expect(replayed.entries_folded).toBe(2);
    expect(changeDetail(replayed).reviews).toHaveLength(1);
  });
});
