//! Drift guard for `web/src/api/types.gen.ts`: with the `ts` feature this test
//! concatenates every web-facing wire type's ts-rs declaration into one module
//! and writes it where the `gen-types` app / `types-drift` check ask (the
//! `TYPES_GEN_OUT` env var). The exact TS shapes come from the types' own
//! `ts`/`serde` attributes and their doc-comments, so a term defined on a wire
//! type reaches the web with it; this file only fixes their order. No
//! `TYPES_GEN_OUT`
//! means a no-op, so `cargo test --features ts` stays read-only.

use ts_rs::{Config, TS};

#[test]
fn write_wire_types() {
    let Some(path) = std::env::var_os("TYPES_GEN_OUT") else {
        return;
    };
    let config = Config::from_env();
    let mut out = String::from(
        "// @generated from crates/nit-types by `nix run .#gen-types` — DO NOT EDIT.\n\
         // Change the Rust wire types, then regenerate.\n\n",
    );
    macro_rules! emit {
        ($($t:ty),* $(,)?) => {$({
            if let Some(docs) = <$t as TS>::docs() {
                out.push_str(&docs);
            }
            out.push_str("export ");
            out.push_str(&<$t as TS>::decl(&config));
            out.push_str("\n\n");
        })*};
    }
    emit!(
        crate::domain::ChangeId,
        crate::domain::Sha,
        crate::domain::RevisionNumber,
        crate::domain::ChangeNumber,
        crate::domain::Side,
        crate::domain::Verdict,
        crate::domain::Decision,
        crate::domain::ChangeStatus,
        crate::domain::ChainState,
        crate::domain::GraphSection,
        crate::domain::FileStatus,
        crate::domain::LineKind,
        crate::domain::DiffMode,
        crate::domain::LifecycleAction,
        crate::repos::Repo,
        crate::repos::RepoList,
        crate::domain::Chain,
        crate::domain::PathEntry,
        crate::graph::RepoGraph,
        crate::graph::GraphNode,
        crate::graph::HistoryCommit,
        crate::graph::RepoHistory,
        crate::changes::ChangeList,
        crate::changes::ChangeDetail,
        crate::changes::ChangeDrafts,
        crate::changes::Revision,
        crate::changes::Review,
        crate::domain::DraftDecision,
        crate::domain::CommentRange,
        crate::comments::Thread,
        crate::comments::ThreadComment,
        crate::domain::Draft,
        crate::comments::NewDraft,
        crate::comments::EditDraft,
        crate::diff::Diff,
        crate::diff::DiffFile,
        crate::diff::FileLines,
        crate::diff::Hunk,
        crate::diff::Line,
        crate::decisions::BatchSubmitResult,
        crate::decisions::SubmitError,
        // The websocket event stream: the change page folds these
        // client-side.
        crate::domain::RevisionPayload,
        crate::domain::ReviewPayload,
        crate::domain::CommentInput,
        crate::domain::LifecyclePayload,
        crate::domain::LogPayload,
        crate::domain::LogEntry,
        crate::events::ClientMessage,
        crate::events::StreamMessage,
        // The folded projection the server ships over the stream; the web
        // holds it opaque and only round-trips it through the wasm fold.
        crate::domain::Lifecycle,
        crate::domain::Anchor,
        crate::domain::RevisionProjection,
        crate::domain::ThreadComment,
        crate::domain::ThreadProjection,
        crate::domain::ReviewProjection,
        crate::domain::ChangeProjection,
    );
    std::fs::write(path, out).expect("write types.gen.ts");
}
