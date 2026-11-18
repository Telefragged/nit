import { atom, read, update } from "claude-code";
import type { EngineInterface as Engine, Register } from "claude-code";

import type { Badge } from "../types";

const DEFAULT_SERVER = "http://127.0.0.1:8877";

const badge = atom({ plugin: "nit", key: "badge" } as const, null);

type Change = {
  id: number;
  change_id: string;
  lifecycle: "active" | "merged" | "abandoned";
  revisions: { message: string }[];
};

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
 * Returns the oldest commit of the current branch that is an active nit
 * change, or `null` when the branch has none.
 *
 * The branch is the commits from the repo's canonical ref to HEAD. Each
 * commit matches a change by its `Change-Id` trailer, so a commit amended
 * after its last push still matches.
 */
async function firstChange($: Engine): Promise<Badge | null> {
  const branch = await git($, ["branch", "--show-current"]);
  const commonDir = await git($, [
    "rev-parse",
    "--path-format=absolute",
    "--git-common-dir",
  ]);
  if (!branch || !commonDir) {
    return null;
  }
  // The server registers a repo by its canonical git common dir.
  const { realPath } = await $.fs.stat(commonDir, { resolve: true });
  const server = (await $.env.get("NIT_SERVER")) ?? DEFAULT_SERVER;
  const { repos } = await getJson<{
    repos: { id: number; git_dir: string; canonical_ref: string }[];
  }>($, `${server}/api/repos`);
  const repo = repos.find((r) => r.git_dir === realPath);
  if (!repo) {
    return null;
  }
  const tag = encodeURIComponent(`branch=${branch}`);
  const { changes } = await getJson<{ changes: Change[] }>(
    $,
    `${server}/api/changes?repo=${repo.id}&tag=${tag}`,
  );
  const active = new Map(
    changes
      .filter((c) => c.lifecycle === "active")
      .map((c) => [c.change_id, c]),
  );
  const log = await git($, [
    "log",
    "--format=%(trailers:key=Change-Id,valueonly,separator=)",
    `${repo.canonical_ref}..HEAD`,
  ]);
  const first = (log ?? "")
    .split("\n")
    .map((changeId) => active.get(changeId))
    .filter((change) => change !== undefined)
    .at(-1);
  if (!first) {
    return null;
  }
  return {
    change: first.id,
    subject: first.revisions.at(-1)?.message.split("\n")[0] ?? "",
    url: `${server}/changes/${first.id}`,
  };
}

async function refresh($: Engine): Promise<void> {
  const shown = await firstChange($).catch(() => null);
  await update($, badge, () => shown);
}

export const register: Register = (on) => {
  // The plugin's SessionStart hook runs `nit watch`, which starts a turn
  // for each review. So the start and the end of each turn cover every
  // change to the branch's changes, and nothing has to poll.
  on("session.start", async ($, e, next) => {
    await refresh($);
    return next(e);
  });
  on("turn.start", async ($, e, next) => {
    if (e.agentId === undefined) {
      await refresh($);
    }
    return next(e);
  });
  on("turn.complete", async ($, e, next) => {
    if (e.agentId === undefined) {
      await refresh($);
    }
    return next(e);
  });

  on("ui.render", { component: "AbovePrompt" }, async ($, e, next) => {
    const shown = await read($, badge);
    if (e.props.hasSurvey || shown === null) {
      return next(e);
    }
    const { Link } = $.ui.resolve(e);
    return (
      <Link href={shown.url} label={`nit #${shown.change} ${shown.subject}`} />
    );
  });
};
