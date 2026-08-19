//! Change endpoints: the change detail and the revision diff (incl. interdiff).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::{Repository, Tree};
use serde::Deserialize;

use nit_types::changes::{ChangeDetail, ChangeDrafts, ChangeList};
use nit_types::diff::{Diff, FileLines};
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeStatus;
use nit_types::domain::DiffMode;
use nit_types::domain::RevisionNumber;
use nit_types::domain::RevisionProjection;
use nit_types::domain::Sha;

use super::diff;
use super::rebase;
use super::views;
use super::{AppPath, AppQuery, AppState, ChangeEntry, Error, with_conn};
use super::{change_detail_json, change_or_404};

#[derive(Deserialize)]
pub(super) struct ListChangesQuery {
    repo: Option<u64>,
    /// Repeated (`?status=pending&status=commented`); empty means every
    /// change — no default subset.
    #[serde(default)]
    status: Vec<ChangeStatus>,
}

/// Serves `GET /api/changes`: matching changes as folded projections.
///
/// `nit_types::changes::ChangeList` carries the filter semantics.
pub(super) async fn list_changes(
    State(state): State<Arc<AppState>>,
    AppQuery(q): AppQuery<ListChangesQuery>,
) -> Result<Json<ChangeList>, Error> {
    with_conn(state.pool(), move |conn| {
        let mut changes = Vec::new();
        for repo_id in state.repo_ids_matching(q.repo) {
            changes.extend(state.repo_changes(conn, repo_id, &q.status)?);
        }
        Ok(Json(ChangeList { changes }))
    })
    .await
}

pub(super) async fn get_change_detail(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
) -> Result<Json<ChangeDetail>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        change_detail_json(conn, &entry)
    })
    .await
}

/// `GET /api/changes/{id}/drafts` — the reviewer's private overlay.
///
/// Drafts plus the draft decision. The change page reads this over REST
/// and the folded projection over the websocket.
pub(super) async fn get_change_drafts(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
) -> Result<Json<ChangeDrafts>, Error> {
    with_conn(state.pool(), move |conn| {
        change_or_404(&state, conn, id)?;
        Ok(Json(views::change_overlay(conn, id)?))
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct DiffQuery {
    against: Option<RevisionNumber>,
    /// `raw` when the request is silent.
    #[serde(default)]
    mode: DiffMode,
}

pub(super) async fn revision_diff(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(ChangeNumber, RevisionNumber)>,
    AppQuery(q): AppQuery<DiffQuery>,
) -> Result<Json<Diff>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        let revs = resolve_revs(&state, &entry, n, q.against)?;
        let mut wire = contained_diff(&revs, 3, q.mode, None)?;
        // After tagging: the message is not a git delta, so it is never drift.
        wire.files.insert(
            0,
            diff::commit_msg_file(
                revs.against.as_ref().map(|a| a.message.as_str()),
                &revs.revision.message,
            ),
        );
        Ok(Json(wire))
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct LinesQuery {
    path: String,
    /// The file's name on the old side, when a rename made the two differ —
    /// what `/diff` reported as its `old_path`.
    ///
    /// Both names bound the tree diffs this request takes, and a rename is
    /// paired only when the bound holds both of its ends: named by its new
    /// side alone, a renamed file would come back as a whole-file add.
    old_path: Option<String>,
    against: Option<RevisionNumber>,
}

/// The whole of file `path`, as diff lines.
///
/// Lets the UI reveal the unchanged runs the shown diff hides. Built from
/// the **same** `old → new` trees and drift tagging as [`revision_diff`],
/// so a revealed line carries the exact kind/drift it would inside a
/// hunk; the client slices the gap it needs.
pub(super) async fn revision_lines(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(ChangeNumber, RevisionNumber)>,
    AppQuery(q): AppQuery<LinesQuery>,
) -> Result<Json<FileLines>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        let revs = resolve_revs(&state, &entry, n, q.against)?;
        let wanted = Wanted {
            path: q.path,
            old_path: q.old_path,
        };
        let wire = contained_diff(&revs, u32::MAX, DiffMode::Raw, Some(&wanted))?;
        let lines = wire
            .files
            .into_iter()
            .find(|f| f.path == wanted.path)
            .map(|f| f.hunks.into_iter().flat_map(|h| h.lines).collect())
            .unwrap_or_default();
        Ok(Json(FileLines { lines }))
    })
    .await
}

/// The one file an answer is about, under every name it goes by.
///
/// The two names are not interchangeable, and each bounds a different thing:
/// both of them bound the tree diff, because a rename is paired only when
/// the bound holds its two ends, while only `path` is worth rendering — an
/// unpaired rename's other side would be read and diffed for nothing.
struct Wanted {
    path: String,
    old_path: Option<String>,
}

impl Wanted {
    /// The pathspec that bounds a tree diff to this file.
    fn names(&self) -> Vec<String> {
        std::iter::once(self.path.clone())
            .chain(self.old_path.clone())
            .collect()
    }
}

/// A revision and an optional interdiff counterpart.
///
/// Cloned out from under the projection read lock so the git work holds
/// nothing live.
struct Revs {
    git_dir: String,
    revision: RevisionProjection,
    against: Option<RevisionProjection>,
}

fn resolve_revs(
    state: &AppState,
    entry: &ChangeEntry,
    n: RevisionNumber,
    against: Option<RevisionNumber>,
) -> Result<Revs, Error> {
    let proj = entry.read();
    let find = |k: RevisionNumber| {
        proj.revision(k)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("revision {k} not found")))
    };
    Ok(Revs {
        git_dir: state.git_dir(proj.repo_id)?,
        revision: find(n)?,
        against: against.map(find).transpose()?,
    })
}

/// The wire diff for `revs` with rebase drift contained.
///
/// `parent → commit` of the revision, or `tree(m) → tree(n)` when it names
/// a counterpart to diff against. Owning the choice here is what keeps
/// `/diff` and `/lines` from having to agree on it separately.
///
/// `only` narrows the answer to one file, and the git work with it: a
/// single file's request never walks the whole interdiff.
///
/// A plain diff when the two revisions share a parent, and on analysis
/// failure.
fn contained_diff(
    revs: &Revs,
    context: u32,
    mode: DiffMode,
    only: Option<&Wanted>,
) -> Result<Diff, Error> {
    let repo = open_repo(&revs.git_dir)?;
    let revision = &revs.revision;
    let new_tree = commit_tree(&repo, &revision.commit_sha)?;
    let old_tree = commit_tree(
        &repo,
        revs.against
            .as_ref()
            .map_or(&revision.parent_sha, |a| &a.commit_sha),
    )?;
    let names = only.map(Wanted::names);
    let git = diff::git_diff(&repo, &old_tree, &new_tree, names.as_deref())?;
    let shown = |path: &str| only.is_none_or(|w| w.path == path);
    let plain = || diff::render(&repo, &git, context, mode, shown);

    let Some(m) = revs
        .against
        .as_ref()
        .filter(|a| a.parent_sha != revision.parent_sha)
    else {
        return plain().map_err(Error::from);
    };
    rebase::contain(&repo, &git, &at(m), &at(revision), context, mode, shown)
        .or_else(|e| {
            tracing::warn!("rebase-aware interdiff analysis failed; serving plain diff: {e:#}");
            plain()
        })
        .map_err(Error::from)
}

fn at(r: &RevisionProjection) -> rebase::Rev<'_> {
    rebase::Rev {
        commit: &r.commit_sha,
        parent: &r.parent_sha,
    }
}

fn open_repo(git_dir: &str) -> Result<Repository, Error> {
    Repository::open(git_dir)
        .map_err(|e| Error::internal(format!("cannot open the repository: {e}")))
}

fn commit_tree<'r>(repo: &'r Repository, sha: &Sha) -> Result<Tree<'r>, Error> {
    diff::commit_tree(repo, sha).ok_or_else(|| Error::internal(format!("tree for {sha} missing")))
}
