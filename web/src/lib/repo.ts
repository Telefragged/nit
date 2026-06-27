/**
 * The git dir is the repo's identity and display name — no separate name
 * field (docs/api.md "Repos").
 */
export const repoPath = (gitDir: string) => gitDir.replace(/\/\.git\/?$/, "");
