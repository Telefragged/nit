import type { EngineInterface as Engine, Register } from "claude-code";

const DEFAULT_SERVER = "http://127.0.0.1:8877";

/** The statuses of a change that is not merged or abandoned. */
const OPEN = ["pending", "approved", "changes_requested", "commented"]
  .map((status) => `status=${status}`)
  .join("&");

type FirstChange = { number: number; url: string };

let shown: FirstChange | null = null;

/** Returns the trimmed stdout of `git args`, or `null` when git fails. */
async function git($: Engine, args: string[]): Promise<string | null> {
  const { exitCode, stdout } = await $.process.run(["git", ...args]);
  return exitCode === 0 ? stdout.trim() : null;
}

async function getJson<T>($: Engine, url: string): Promise<T> {
  const response = await $.http.fetch(url);
  if (!response.ok) {
    throw new Error(`${url}: HTTP ${response.status}`);
  }
  return JSON.parse(response.text) as T;
}

/**
 * Returns the oldest open nit change among the commits of the current
 * branch, or `null` when the branch has none.
 *
 * The branch is the commits from the repo's canonical ref to HEAD. Each
 * commit matches a change by its `Change-Id` trailer, so a commit amended
 * after its last push still matches.
 */
async function firstChange($: Engine): Promise<FirstChange | null> {
  const [commonDir, server] = await Promise.all([
    git($, ["rev-parse", "--path-format=absolute", "--git-common-dir"]),
    $.env.get("NIT_SERVER").then((value) => value ?? DEFAULT_SERVER),
  ]);
  if (!commonDir) {
    return null;
  }
  // The server registers a repo by its canonical git common dir.
  const [{ realPath }, { repos }] = await Promise.all([
    $.fs.stat(commonDir, { resolve: true }),
    getJson<{
      repos: { id: number; git_dir: string; canonical_ref: string }[];
    }>($, `${server}/api/repos`),
  ]);
  const repo = repos.find((r) => r.git_dir === realPath);
  if (!repo) {
    return null;
  }
  const [{ changes }, log] = await Promise.all([
    getJson<{ changes: { id: number; change_id: string }[] }>(
      $,
      `${server}/api/changes?repo=${repo.id}&${OPEN}`,
    ),
    git($, [
      "log",
      "--format=%(trailers:key=Change-Id,valueonly,separator=)",
      `${repo.canonical_ref}..HEAD`,
    ]),
  ]);
  if (!log) {
    return null;
  }
  const open = new Map(changes.map((c) => [c.change_id, c.id]));
  const number = open.get(
    log.split("\n").findLast((changeId) => open.has(changeId)) ?? "",
  );
  return number === undefined
    ? null
    : { number, url: `${server}/changes/${number}` };
}

async function refresh($: Engine): Promise<void> {
  shown = await firstChange($).catch(() => null);
  $.ui.invalidate("ui.render");
}

export const register: Register = (on) => {
  // The plugin's SessionStart hook runs `nit watch`, which starts a turn
  // for each review. So the end of each turn covers every change to the
  // branch's changes, and nothing has to poll.
  on("session.start", async ($, e, next) => {
    const [result] = await Promise.all([next(e), refresh($)]);
    return result;
  });
  on("turn.complete", async ($, e, next) => {
    if (e.agentId !== undefined) {
      return next(e);
    }
    const [result] = await Promise.all([next(e), refresh($)]);
    return result;
  });

  on("ui.render", { component: "PromptHint" }, async ($, e, next) => {
    if (shown === null) {
      return next(e);
    }
    const { Box, Link, Text } = $.ui.resolve(e);
    return (
      <Box flexDirection="row">
        {await next(e)}
        <Text dimColor> · nit </Text>
        <Link href={shown.url}>
          <Text color="suggestion" underline>
            #{shown.number}
          </Text>
        </Link>
      </Box>
    );
  });
};
