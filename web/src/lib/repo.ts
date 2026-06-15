/**
 * A repo's display path: its git-common-dir with a trailing `/.git` stripped,
 * so `/home/u/acme/.git` shows as `/home/u/acme`. The git dir is the repo's
 * identity and its name — there is no separate name field (docs/api.md
 * "Repos").
 */
export const repoPath = (gitDir: string) => gitDir.replace(/\/\.git\/?$/, "");
