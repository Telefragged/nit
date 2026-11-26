import { describe, expect, it } from "vitest";
import type { Run } from "../api/document.gen";
import { extendedRange, rangeParams, versionRange } from "./documentVersions";

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

  it("selects the one version", () => {
    expect(versionRange(RUNS, "9@1", null)).toEqual({ from: 2, to: 2 });
    expect(versionRange(RUNS, "3@1", null)).toEqual({ from: 0, to: 0 });
  });

  it("starts at an earlier run", () => {
    expect(versionRange(RUNS, "9@1", "3@1")).toEqual({ from: 0, to: 2 });
  });

  it("ignores a base that is not before the version", () => {
    expect(versionRange(RUNS, "7@2", "9@1")).toEqual({ from: 1, to: 1 });
    expect(versionRange(RUNS, "7@2", "7@2")).toEqual({ from: 1, to: 1 });
    expect(versionRange(RUNS, "7@2", "4@1")).toEqual({ from: 1, to: 1 });
  });
});

describe("rangeParams", () => {
  it("is the inverse of versionRange", () => {
    for (const range of [
      { from: 2, to: 2 },
      { from: 0, to: 2 },
      { from: 0, to: 0 },
    ]) {
      const { version, against } = rangeParams(RUNS, range);
      expect(versionRange(RUNS, version, against)).toEqual(range);
    }
  });

  it("has no base for one version", () => {
    expect(rangeParams(RUNS, { from: 2, to: 2 })).toEqual({
      version: "9@1",
      against: null,
    });
  });

  it("selects the live document for no range", () => {
    expect(rangeParams(RUNS, null)).toEqual({ version: null, against: null });
  });
});

describe("extendedRange", () => {
  it("is the one version without a range", () => {
    expect(extendedRange(null, 2)).toEqual({ from: 2, to: 2 });
  });

  it("moves the end to a run at or after the start", () => {
    expect(extendedRange({ from: 0, to: 0 }, 2)).toEqual({ from: 0, to: 2 });
    expect(extendedRange({ from: 0, to: 2 }, 1)).toEqual({ from: 0, to: 1 });
    expect(extendedRange({ from: 0, to: 2 }, 0)).toEqual({ from: 0, to: 0 });
  });

  it("moves the start to a run before the range", () => {
    expect(extendedRange({ from: 2, to: 2 }, 0)).toEqual({ from: 0, to: 2 });
  });
});
