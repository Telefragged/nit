//! Change endpoints: the change list and log, the change detail, its tags,
//! and the revision diff (incl. interdiff).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::{Repository, Tree};
use serde::Deserialize;

use nit_types::changes::{ChangeDetail, ChangeDrafts, ChangeList, ChangeQuery};
use nit_types::changes::{TagList, TagsRequest};
use nit_types::diff::{Diff, FileLines};
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeStatus;
use nit_types::domain::PortedComment;
use nit_types::domain::RevisionNumber;
use nit_types::domain::RevisionProjection;
use nit_types::domain::Sha;
use nit_types::domain::Tag;
use nit_types::domain::ThreadOrigin;
use nit_types::domain::{DiffMode, DiffView, Whitespace};
use nit_types::domain::{LogPayload, TagsPayload};
use nit_types::log::Log;

use crate::db;
use crate::review;

use super::diff;
use super::port;
use super::rebase;
use super::views;
use super::{AppJson, AppPath, AppQuery, AppState, ChangeEntry, Error, with_conn};
use super::{append_to_change, change_detail_json, change_or_404, map_busy};

/// Serves `GET /api/changes`: matching changes as folded projections.
pub(super) async fn list_changes(
    State(state): State<Arc<AppState>>,
    AppQuery(q): AppQuery<ChangeQuery>,
) -> Result<Json<ChangeList>, Error> {
    with_conn(state.pool(), move |conn| {
        Ok(Json(ChangeList {
            changes: state.changes_matching(conn, q)?,
        }))
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct LogQuery {
    repo: u64,
    #[serde(default)]
    status: Vec<ChangeStatus>,
    #[serde(default)]
    tag: Vec<Tag>,
    after: Option<u64>,
    before: Option<u64>,
}

/// Serves `GET /api/log`: the entries of the changes a filter matches.
///
/// `nit_types::log::Log` documents the query. This reads the log rows
/// directly and never looks the repo up, so an unknown repo returns an
/// empty log, not a 404.
pub(super) async fn list_log(
    State(state): State<Arc<AppState>>,
    AppQuery(q): AppQuery<LogQuery>,
) -> Result<Json<Log>, Error> {
    with_conn(state.pool(), move |conn| {
        let filter = db::ChangeFilter {
            statuses: q.status,
            tags: q.tag.into_iter().collect(),
            ..db::ChangeFilter::default()
        };
        let entries = review::entries_between(conn, q.repo, &filter, q.after, q.before)?;
        Ok(Json(Log { entries }))
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct ListTagsQuery {
    repo: u64,
    /// Repeated (`?status=pending&status=commented`); empty means every
    /// change, like the change read.
    #[serde(default)]
    status: Vec<ChangeStatus>,
}

/// Serves `GET /api/tags`: the repo's tags in use, grouped by key.
///
/// `nit_types::changes::TagList` carries the semantics. `repo` is required,
/// because tag keys collide across repos and no union of them means
/// anything. The read goes straight to the denormalized rows, so an
/// unknown repo yields an empty list rather than a 404.
pub(super) async fn list_tags(
    State(state): State<Arc<AppState>>,
    AppQuery(q): AppQuery<ListTagsQuery>,
) -> Result<Json<TagList>, Error> {
    with_conn(state.pool(), move |conn| {
        let mut tags: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (key, value) in db::repo_tags(conn, q.repo, &q.status)? {
            tags.entry(key).or_default().push(value);
        }
        Ok(Json(TagList { tags }))
    })
    .await
}

/// `POST /api/changes/{id}/tags` — puts tags on one change.
///
/// The new tags lay over the ones the change carries, so a key they omit
/// keeps its value. Tags that move no key append nothing, and none of
/// this touches the change's revisions or its review status.
pub(super) async fn tag_change(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
    AppJson(req): AppJson<TagsRequest>,
) -> Result<Json<ChangeDetail>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        let moves = {
            let proj = entry.read();
            !proj.tags.carries_all(&req.tags)
        };
        if moves {
            let new = LogPayload::Tags(TagsPayload { tags: req.tags });
            append_to_change(&state, conn, &entry, id, vec![new]).map_err(map_busy)?;
        }
        change_detail_json(conn, &entry)
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
    #[serde(flatten)]
    view: DiffView,
}

pub(super) async fn revision_diff(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(ChangeNumber, RevisionNumber)>,
    AppQuery(q): AppQuery<DiffQuery>,
) -> Result<Json<Diff>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        let revs = resolve_revs(&state, &entry, n, q.against)?;
        let mut wire = contained_diff(&revs, 3, q.view, None)?;
        // After tagging: the message is not a git delta, so it is never drift.
        wire.files.insert(
            0,
            diff::commit_msg_file(
                revs.against.as_ref().map(|a| a.message.as_str()),
                &revs.revision.message,
                3,
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
    /// How the diff this reveal happens inside compares whitespace.
    ///
    /// A revealed line then carries the kind it would hold in a hunk.
    #[serde(default)]
    whitespace: Whitespace,
}

/// The whole of file `path`, as diff lines.
///
/// Lets the UI reveal the unchanged runs the shown diff hides. Built from
/// the **same** `old → new` trees and drift tagging as [`revision_diff`],
/// so a revealed line carries the exact kind/drift it would inside a
/// hunk; the client slices the gap it needs. The synthetic
/// [`diff::COMMIT_MSG_PATH`] answers with the whole commit message, as
/// [`revision_diff`] compares it.
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
        let view = DiffView {
            mode: DiffMode::Full,
            whitespace: q.whitespace,
        };
        let file = if wanted.path == diff::COMMIT_MSG_PATH {
            Some(diff::commit_msg_file(
                revs.against.as_ref().map(|a| a.message.as_str()),
                &revs.revision.message,
                u32::MAX,
            ))
        } else {
            contained_diff(&revs, u32::MAX, view, Some(&wanted))?
                .files
                .into_iter()
                .find(|f| f.path == wanted.path)
        };
        let lines = file
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
    view: DiffView,
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
    let plain = || diff::render(&repo, &git, context, view, shown);

    let mut wire = match revs
        .against
        .as_ref()
        .filter(|a| a.parent_sha != revision.parent_sha)
    {
        None => plain()?,
        Some(m) => rebase::contain(&repo, &git, &at(m), &at(revision), context, view, shown)
            .or_else(|e| {
                tracing::warn!("rebase-aware interdiff analysis failed; serving plain diff: {e:#}");
                plain()
            })?,
    };
    if view.mode == DiffMode::Outline {
        // An outline answers with outlines, so a file that has none to show
        // is not in it — whatever else its delta did. A change inside a
        // body, a rename, a binary blob: each leaves a row saying nothing,
        // which is the noise the mode exists to remove.
        wire.files.retain(|file| !file.hunks.is_empty());
    }
    Ok(wire)
}

#[derive(Deserialize)]
pub(super) struct PortedQuery {
    against: Option<RevisionNumber>,
    #[serde(default)]
    include_resolved: bool,
}

/// `GET /api/changes/{id}/revisions/{n}/ported`.
///
/// Ports every unresolved thread of a revision earlier than `n` to `n`,
/// and with `?against={m}` to `m` as well, `n`'s entries first.
/// `?include_resolved=true` ports the resolved threads too. Entries for
/// one revision are sorted by thread id.
pub(super) async fn ported_comments(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(ChangeNumber, RevisionNumber)>,
    AppQuery(q): AppQuery<PortedQuery>,
) -> Result<Json<Vec<PortedComment>>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        let revs = resolve_revs(&state, &entry, n, q.against)?;
        // Only a thread of an earlier revision can port to either target.
        let latest = revs
            .against
            .as_ref()
            .map_or(revs.revision.number, |m| m.number.max(revs.revision.number));
        let (revisions, threads) = {
            let proj = entry.read();
            let threads: Vec<_> = proj
                .threads
                .iter()
                .filter(|t| t.revision < latest && (q.include_resolved || !t.resolved))
                .map(|t| ThreadOrigin {
                    thread_id: t.id,
                    revision: t.revision,
                    anchor: t.anchor.clone(),
                })
                .collect();
            (proj.revisions.clone(), threads)
        };
        let repo = open_repo(&revs.git_dir)?;
        let targets: Vec<_> = std::iter::once(&revs.revision)
            .chain(revs.against.as_ref())
            .collect();
        Ok(Json(port::port_threads(
            &repo, &revisions, &targets, &threads,
        )?))
    })
    .await
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A flattened field makes serde buffer the whole query before it reads
    /// any of it, and a buffered number arrives as a string. The parser has
    /// to convert it back, or `against` fails as soon as another field is
    /// flattened beside it.
    #[test]
    fn a_diff_request_reads_its_revision_beside_its_view() {
        let q: DiffQuery = serde_html_form::from_str("against=1&mode=outline&whitespace=ignore")
            .expect("query parses");
        assert_eq!(q.against.map(RevisionNumber::get), Some(1));
        assert_eq!(
            q.view,
            DiffView {
                mode: DiffMode::Outline,
                whitespace: Whitespace::Ignore,
            }
        );

        let silent: DiffQuery = serde_html_form::from_str("").expect("empty query parses");
        assert_eq!(silent.view, DiffView::default());
    }
}
