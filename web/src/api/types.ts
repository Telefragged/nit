// The web's wire types are GENERATED from crates/nit-types into ./types.gen.ts
// (regenerate with `nix run .#gen-types`); collecting them here gives the app
// one import point, alongside client-only constants with no Rust counterpart.

export type * from "./types.gen";

/**
 * Reserved synthetic diff path: the revision's commit message as a
 * reviewable file, listed first in every diff.
 */
export const COMMIT_MSG_PATH = "/COMMIT_MSG";

/**
 * Reserved prefix of an attachment's synthetic diff path: an attachment
 * named `plan.md` is the file `/attachments/plan.md`. A git path never
 * starts with `/`, so it cannot collide with a tracked file.
 */
export const ATTACHMENT_PREFIX = "/attachments/";

/** The largest attachment that shows as lines, in bytes. The server reads
 * no larger blob as text, so a thread on a line of one could not keep its
 * line text or port. */
export const MAX_TEXT_SIZE = 1024 * 1024;
