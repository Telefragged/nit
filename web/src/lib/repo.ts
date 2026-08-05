/**
 * The git dir is the repo's identity and display name — no separate name
 * field.
 */
export const repoPath = (gitDir: string) => gitDir.replace(/\/\.git\/?$/, "");
