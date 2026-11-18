import type { On } from "claude-code";
import { expect, test } from "claude-code/testing";

const GIT: Record<string, string> = {
  "rev-parse --path-format=absolute --git-common-dir": "/w/nit/.git\n",
  // Newest first: an unpushed commit, two pushed changes, then a merged one.
  "log --format=%(trailers:key=Change-Id,valueonly,separator=) main..HEAD":
    "\nItop\nIbottom\nImerged\n",
};

const REPOS = "http://127.0.0.1:8877/api/repos";
const OPEN_CHANGES =
  "http://127.0.0.1:8877/api/changes?repo=3&status=pending&status=approved&status=changes_requested&status=commented";

/**
 * Stubs git, the filesystem, the environment, the nit server and the
 * session events.
 *
 * `server.isBottomOpen` sets whether change 8, the oldest open one, is
 * still open, and `null` makes the server unreachable.
 */
function host(on: On, server: { isBottomOpen: boolean } | null) {
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
  on("fs.stat", (_, e) => ({
    value: {
      kind: "dir",
      size: 0,
      mtimeMs: 0,
      isLink: false,
      realPath: e.path,
    },
  }));
  on("env.get", () => ({ value: undefined }));
  on("http.fetch", (_, e) => {
    if (server === null) {
      throw new Error("connection refused");
    }
    const open = [{ id: 9, change_id: "Itop" }];
    if (server.isBottomOpen) {
      open.push({ id: 8, change_id: "Ibottom" });
    }
    const body: Record<string, unknown> = {
      [REPOS]: {
        repos: [{ id: 3, git_dir: "/w/nit/.git", canonical_ref: "main" }],
      },
      [OPEN_CHANGES]: { changes: open },
    };
    const text = JSON.stringify(body[e.url] ?? {});
    return { value: { status: 200, ok: true, headers: {}, text } };
  });
  on("session.start", (_, e) => ({ cwd: e.cwd }));
  on("turn.complete", () => ({ text: "" }));
}

const HINT = {
  plugin: "nit",
  surface: "terminal",
  component: "PromptHint",
  props: { isDraft: false, isWorking: false, hint: "? for shortcuts" },
} as const;

const START = {
  cwd: "/w/nit",
  surface: "terminal",
  isInteractive: true,
} as const;

test("the footer links the oldest open change of the branch", async ($, on) => {
  host(on, { isBottomOpen: true });
  await $.session.start(START);

  const ui = await $.ui.mount(HINT);
  expect((await ui.find({ type: "Link" }))?.props.href).toBe(
    "http://127.0.0.1:8877/changes/8",
  );
  expect(await ui.find({ type: "Text", text: "#8" })).toBeDefined();
  await ui.unmount();
});

test("the link moves to the next change when a turn ends after a merge", async ($, on) => {
  const server = { isBottomOpen: true };
  host(on, server);
  await $.session.start(START);
  server.isBottomOpen = false;
  await $.turn.complete({
    reason: "answer",
    answer: "",
    durationMs: 0,
    isAborted: false,
    turnId: "t0",
  });

  const ui = await $.ui.mount(HINT);
  expect((await ui.find({ type: "Link" }))?.props.href).toBe(
    "http://127.0.0.1:8877/changes/9",
  );
  await ui.unmount();
});

test("the footer has no link when the server is unreachable", async ($, on) => {
  host(on, null);
  await $.session.start(START);

  const ui = await $.ui.mount(HINT);
  expect(await ui.find({ type: "Link" })).toBeUndefined();
  await ui.unmount();
});
