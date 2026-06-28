// The web's wire types are GENERATED from crates/nit-types into ./types.gen.ts
// (regenerate with `nix run .#gen-types`); collecting them here gives the app
// one import point, alongside client-only constants with no Rust counterpart.

export type * from "./types.gen";

/**
 * Reserved synthetic diff path: the revision's commit message as a
 * reviewable file, listed first in every diff (docs/api.md "The commit
 * message as a file").
 */
export const COMMIT_MSG_PATH = "/COMMIT_MSG";
