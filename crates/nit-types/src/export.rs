//! Drift guard for `web/src/api/types.gen.ts`: with the `ts` feature this test
//! concatenates every web-facing wire type's ts-rs declaration into one module
//! and writes it where the `gen-types` app / `types-drift` check ask (the
//! `TYPES_GEN_OUT` env var). The exact TS shapes come from the types' own
//! `ts`/`serde` attributes; this file only fixes their order. No `TYPES_GEN_OUT`
//! means a no-op, so `cargo test --features ts` stays read-only.

use ts_rs::{Config, TS};

#[test]
fn write_wire_types() {
    let Some(path) = std::env::var_os("TYPES_GEN_OUT") else {
        return;
    };
    let cfg = Config::from_env();
    let mut out = String::from(
        "// @generated from crates/nit-types by `nix run .#gen-types` — DO NOT EDIT.\n\
         // Change the Rust wire types (and docs/api.md), then regenerate.\n\n",
    );
    macro_rules! emit {
        ($($t:ty),* $(,)?) => {$({
            out.push_str("export ");
            out.push_str(&<$t as TS>::decl(&cfg));
            out.push_str("\n\n");
        })*};
    }
    emit!(
        crate::enums::Side,
        crate::enums::Verdict,
        crate::enums::Decision,
        crate::enums::ChangeStatus,
        crate::enums::ChainState,
        crate::enums::GraphSection,
        crate::enums::FileStatus,
        crate::enums::LineKind,
        crate::repos::Repo,
        crate::repos::RepoList,
        crate::chains::Chain,
        crate::chains::PathEntry,
        crate::graph::RepoGraph,
        crate::graph::GraphNode,
        crate::changes::ChangeDetail,
        crate::changes::Revision,
        crate::changes::Review,
        crate::changes::StagedDecision,
        crate::comments::CommentRange,
        crate::comments::Thread,
        crate::comments::ThreadComment,
        crate::comments::Draft,
        crate::comments::NewDraft,
        crate::comments::EditDraft,
        crate::diff::Diff,
        crate::diff::DiffFile,
        crate::diff::FileLines,
        crate::diff::Hunk,
        crate::diff::Line,
        crate::decisions::BatchSubmitResult,
        crate::decisions::SubmitError,
    );
    std::fs::write(path, out).expect("write types.gen.ts");
}
