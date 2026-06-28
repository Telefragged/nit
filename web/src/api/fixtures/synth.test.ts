import { describe, expect, it } from "vitest";

import { changeDetail, replayProj } from "../fold";
import { changes, threads } from "./data";
import { synthLog } from "./synth";

// The synth log is the mock's single source of truth: folding it (via the real
// wasm, as both the snapshot and the REST read do) must reproduce the change
// records it was synthesized from — otherwise the two would disagree.
describe("synthLog fidelity", () => {
  const sortById = <T extends { id: number }>(xs: T[]) =>
    [...xs].sort((a, b) => a.id - b.id);

  for (const change of changes) {
    it(`folds back to change ${change.id}'s published state`, () => {
      const recThreads = threads.filter((t) => t.change_id === change.id);
      const folded = changeDetail(
        replayProj({
          id: change.id,
          repo_id: change.repo_id,
          change_key: change.change_key,
          entries: synthLog(change, recThreads),
        }),
      );

      expect(folded.revisions.map((r) => r.commit_sha)).toEqual(
        change.revisions.map((r) => r.commit_sha),
      );
      expect(sortById(folded.reviews).map((r) => [r.id, r.verdict])).toEqual(
        sortById(change.reviews).map((r) => [r.id, r.verdict]),
      );

      const shape = (t: {
        id: number;
        file: string | null;
        line: number | null;
        resolved: boolean;
        comments: { body: string; review_id: number | null }[];
      }) => ({
        id: t.id,
        file: t.file,
        line: t.line,
        resolved: t.resolved,
        comments: t.comments.map((c) => [c.body, c.review_id]),
      });
      expect(sortById(folded.threads).map(shape)).toEqual(
        sortById(recThreads).map(shape),
      );
    });
  }
});
