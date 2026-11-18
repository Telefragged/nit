import type { On } from "claude-code";
import { expect, test } from "claude-code/testing";

const BAND = {
  plugin: "nit",
  surface: "terminal",
  component: "AbovePrompt",
  props: {
    hasSurvey: false,
    isCollapsed: false,
    isWorking: false,
    maxRows: 10,
    bodyColumns: 80,
    scroll: { offset: 0, bodyRows: 9 },
    view: {},
  },
} as const;

const GIT: Record<string, string> = {
  "branch --show-current": "track/badge\n",
  "rev-parse --path-format=absolute --git-common-dir":
    "/w/nit/.worktrees/x/../../.git\n",
  // Newest first: an unpushed commit, two pushed changes, then a merged one.
  "log --format=%(trailers:key=Change-Id,valueonly,separator=) main..HEAD":
    "\nItop\nIbottom\nImerged\n",
};

const REPOS = "http://127.0.0.1:8877/api/repos";
const CHANGES =
  "http://127.0.0.1:8877/api/changes?repo=3&tag=branch%3Dtrack%2Fbadge";

const change = (
  id: number,
  changeId: string,
  lifecycle: string,
  message: string,
) => ({
  id,
  change_id: changeId,
  lifecycle,
  revisions: [{ message: "old subject" }, { message }],
});

/**
 * Answers git, the filesystem, the environment and the nit server.
 *
 * `server` sets the lifecycle of change 8, the oldest active one, and
 * `null` makes the server unreachable.
 */
function host(on: On, server: { lifecycle: string } | null) {
  on("process.run", (_, e) => {
    const stdout = GIT[e.argv.slice(1).join(" ")];
    return {
      value: {
        exitCode: stdout === undefined ? 1 : 0,
        stdout: stdout ?? "",
        stderr: "",
        isStdoutTruncated: false,
        isStderrTruncated: false,
      },
    };
  });
  on("fs.stat", () => ({
    value: {
      kind: "dir",
      size: 0,
      mtimeMs: 0,
      isLink: false,
      realPath: "/w/nit/.git",
    },
  }));
  on("env.get", () => ({ value: undefined }));
  on("http.fetch", (_, e) => {
    if (server === null) {
      throw new Error("connection refused");
    }
    const body: Record<string, unknown> = {
      [REPOS]: {
        repos: [{ id: 3, git_dir: "/w/nit/.git", canonical_ref: "main" }],
      },
      [CHANGES]: {
        changes: [
          change(7, "Imerged", "merged", "cli: merged"),
          change(9, "Itop", "active", "web: top"),
          change(8, "Ibottom", server.lifecycle, "cli: bottom\n\nbody"),
        ],
      },
    };
    const text = JSON.stringify(body[e.url] ?? {});
    return { value: { status: 200, ok: true, headers: {}, text } };
  });
  on("session.start", (_, e) => ({ cwd: e.cwd }));
  on("turn.start", (_, e) => ({ turnId: e.turnId }));
}

test("the band links the oldest active change of the branch", async ($, on) => {
  host(on, { lifecycle: "active" });
  await $.session.start({
    cwd: "/w/nit",
    surface: "terminal",
    isInteractive: true,
  });

  const ui = await $.ui.mount(BAND);
  expect((await ui.find({ type: "Link" }))?.props).toEqual({
    href: "http://127.0.0.1:8877/changes/8",
    label: "nit #8 cli: bottom",
  });
  await ui.unmount();
});

test("the band moves to the next change when a turn starts after a merge", async ($, on) => {
  const server = { lifecycle: "active" };
  host(on, server);
  await $.session.start({
    cwd: "/w/nit",
    surface: "terminal",
    isInteractive: true,
  });
  server.lifecycle = "merged";
  await $.turn.start({
    text: "A reviewer acted on your changes.",
    turnId: "t0",
  });

  const ui = await $.ui.mount(BAND);
  expect((await ui.find({ type: "Link" }))?.props.label).toBe(
    "nit #9 web: top",
  );
  await ui.unmount();
});

test("the band is empty when the server is unreachable", async ($, on) => {
  host(on, null);
  await $.session.start({
    cwd: "/w/nit",
    surface: "terminal",
    isInteractive: true,
  });

  const ui = await $.ui.mount(BAND);
  expect(await ui.find({ type: "Link" })).toBeUndefined();
  await ui.unmount();
});
