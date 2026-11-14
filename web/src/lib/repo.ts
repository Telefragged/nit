/**
 * The git dir is the repo's identity and display name.
 */
export const repoPath = (gitDir: string) => gitDir.replace(/\/\.git\/?$/, "");
