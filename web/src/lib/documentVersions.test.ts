import { describe, expect, it } from "vitest";
import type { Run } from "../api/document.gen";
import { EMPTY, rangeParams, versionRange } from "./documentVersions";

const run = (version: string): Run => ({
  writer: "author",
  version,
  at: "2026-01-01T09:00:00Z",
});
const RUNS = [run("3@1"), run("7@2"), run("9@1")];

describe("versionRange", () => {
  it("selects no range for a version of no run", () => {
    expect(versionRange(RUNS, null, null)).toBeNull();
    expect(versionRange(RUNS, "4@1", "3@1")).toBeNull();
  });

  it("starts at the run before the version", () => {
    expect(versionRange(RUNS, "9@1", null)).toEqual({ to: 2, from: 1 });
    expect(versionRange(RUNS, "3@1", null)).toEqual({ to: 0, from: null });
  });

  it("starts at an earlier run, or at the empty text", () => {
    expect(versionRange(RUNS, "9@1", "3@1")).toEqual({ to: 2, from: 0 });
    expect(versionRange(RUNS, "9@1", EMPTY)).toEqual({ to: 2, from: null });
  });

  it("ignores a base that is not before the version", () => {
    expect(versionRange(RUNS, "7@2", "9@1")).toEqual({ to: 1, from: 0 });
    expect(versionRange(RUNS, "7@2", "7@2")).toEqual({ to: 1, from: 0 });
    expect(versionRange(RUNS, "7@2", "5@5")).toEqual({ to: 1, from: 0 });
  });
});

describe("rangeParams", () => {
  it("removes both parameters for the live document", () => {
    expect(rangeParams(RUNS, null)).toEqual({ version: null, against: null });
  });

  it("omits the base that is the run before the version", () => {
    expect(rangeParams(RUNS, { to: 2, from: 1 })).toEqual({
      version: "9@1",
      against: null,
    });
    expect(rangeParams(RUNS, { to: 0, from: null })).toEqual({
      version: "3@1",
      against: null,
    });
  });

  it("gives the range that versionRange selects", () => {
    for (const range of [
      { to: 2, from: 0 },
      { to: 2, from: null },
      { to: 1, from: 0 },
    ]) {
      const { version, against } = rangeParams(RUNS, range);
      expect(versionRange(RUNS, version, against)).toEqual(range);
    }
  });
});
