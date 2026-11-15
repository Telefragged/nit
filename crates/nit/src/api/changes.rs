//! Change endpoints: the change list and log, the change detail, its tags,
//! and the revision diff (incl. interdiff).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::{Repository, Tree};
use rusqlite::Connection;
use serde::Deserialize;

use nit_types::changes::{ChangeDetail, ChangeDrafts, ChangeList, ChangeQuery};
use nit_types::changes::{TagList, TagsRequest};
use nit_types::diff::{Diff, FileLines};
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeProjection;
use nit_types::domain::ChangeStatus;
use nit_types::domain::ChangeVersion;
use nit_types::domain::PortedComment;
use nit_types::domain::RevisionNumber;
use nit_types::domain::RevisionProjection;
use nit_types::domain::Sha;
use nit_types::domain::ThreadOrigin;
use nit_types::domain::Whitespace;
use nit_types::domain::{LogPayload, TagsPayload};
use nit_types::log::Log;

use crate::db;
use crate::review;

use super::diff;
use super::port;
use super::rebase;
use super::state::{changes_matching, git_dir};
use super::views;
use super::{AppJson, AppPath, AppQuery, AppState, Error, with_conn};
use super::{append, change_or_404};

/// Serves `GET /api/changes`: matching changes as folded projections.
#[utoipa::path(get, path = "/api/changes", params(ChangeQuery), responses((status = 200, body = ChangeList)))]
pub(super) async fn list_changes(
    State(state): State<Arc<AppState>>,
    AppQuery(q): AppQuery<ChangeQuery>,
) -> Result<Json<ChangeList>, Error> {
    with_conn(state.pool(), move |conn| {
        Ok(Json(ChangeList {
            changes: changes_matching(conn, q)?,
        }))
    })
    .await
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(super) struct ChangeLogQuery {
    #[serde(default)]
    from: u64,
    to: Option<u64>,
}

/// Serves `GET /api/changes/{id}/log`: the entries of a change in a range
/// of positions.
///
/// `nit_types::log::Log` documents the query.
#[utoipa::path(get, path = "/api/changes/{id}/log", params(("id" = ChangeNumber, Path), ChangeLogQuery), responses((status = 200, body = Log)))]
pub(super) async fn get_change_log(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
    AppQuery(q): AppQuery<ChangeLogQuery>,
) -> Result<Json<Log>, Error> {
    with_conn(state.pool(), move |conn| {
        if db::get_change(conn, id)?.is_none() {
            return Err(Error::not_found(format!("change {id} not found")));
        }
        Ok(Json(Log {
            entries: review::entries_from(conn, id, q.from, q.to)?,
        }))
    })
    .await
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(super) struct ListTagsQuery {
    repo: u64,
    /// Repeated (`?status=pending&status=commented`); empty selects every
    /// change, as in `GET /api/changes`.
    #[serde(default)]
    status: Vec<ChangeStatus>,
}

/// Serves `GET /api/tags`: the repo's tags in use, grouped by key.
///
/// `nit_types::changes::TagList` documents the semantics. `repo` is
/// required, because tag keys collide across repos. The query goes
/// straight to the denormalized rows, so an unknown repo yields an empty
/// list.
#[utoipa::path(get, path = "/api/tags", params(ListTagsQuery), responses((status = 200, body = TagList)))]
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
/// The new tags lay over the ones the change has, so a key they omit
/// keeps its value. Tags that move no key append nothing, and none of
/// this touches the change's revisions or its review status.
#[utoipa::path(post, path = "/api/changes/{id}/tags", params(("id" = ChangeNumber, Path)), request_body = TagsRequest, responses((status = 200, body = ChangeDetail)))]
pub(super) async fn tag_change(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
    AppJson(req): AppJson<TagsRequest>,
) -> Result<Json<ChangeDetail>, Error> {
    with_conn(state.pool(), move |conn| {
        let appended = append(&state, conn, id, |change, _| {
            Ok(if change.tags.carries_all(&req.tags) {
                Vec::new()
            } else {
                vec![LogPayload::Tags(TagsPayload { tags: req.tags })]
            })
        })?;
        Ok(Json(views::build_change_detail(conn, &appended.change)?))
    })
    .await
}

#[utoipa::path(get, path = "/api/changes/{id}", params(("id" = ChangeNumber, Path)), responses((status = 200, body = ChangeDetail)))]
pub(super) async fn get_change_detail(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
) -> Result<Json<ChangeDetail>, Error> {
    with_conn(state.pool(), move |conn| {
        Ok(Json(views::build_change_detail(
            conn,
            &change_or_404(conn, id)?,
        )?))
    })
    .await
}

/// `GET /api/changes/{id}/drafts` — the reviewer's private overlay.
///
/// Drafts plus the draft decision. The change page loads this over REST
/// and the folded projection over the websocket.
#[utoipa::path(get, path = "/api/changes/{id}/drafts", params(("id" = ChangeNumber, Path)), responses((status = 200, body = ChangeDrafts)))]
pub(super) async fn get_change_drafts(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
) -> Result<Json<ChangeDrafts>, Error> {
    with_conn(state.pool(), move |conn| {
        change_or_404(conn, id)?;
        Ok(Json(views::change_overlay(conn, id)?))
    })
    .await
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(super) struct DiffQuery {
    /// The old side of the diff: a revision, or the base of revision `n`,
    /// which is also the default.
    against: Option<ChangeVersion>,
    #[serde(default)]
    whitespace: Whitespace,
}

#[utoipa::path(get, path = "/api/changes/{id}/revisions/{n}/diff", params(("id" = ChangeNumber, Path), ("n" = RevisionNumber, Path), DiffQuery), responses((status = 200, body = Diff)))]
pub(super) async fn revision_diff(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(ChangeNumber, RevisionNumber)>,
    AppQuery(q): AppQuery<DiffQuery>,
) -> Result<Json<Diff>, Error> {
    with_conn(state.pool(), move |conn| {
        let change = change_or_404(conn, id)?;
        let revs = resolve_revs(conn, &change, n, q.against)?;
        let mut wire = contained_diff(&revs, 3, q.whitespace, None)?;
        let shown = |version| -> BTreeMap<_, _> {
            change
                .effective_attachments(version)
                .into_iter()
                .map(|a| (a.name, a.hash))
                .collect()
        };
        let (old, new) = (shown(revs.old_version()), shown(ChangeVersion::Revision(n)));
        // An attachment that both sides show with the same bytes stays
        // out of the diff, as an unchanged file does.
        let names: BTreeSet<_> = old
            .keys()
            .chain(new.keys())
            .filter(|name| old.get(*name) != new.get(*name))
            .collect();
        wire.files.extend(names.into_iter().map(|name| {
            diff::attachment_file(
                state.blobs(),
                name.as_str(),
                old.get(name),
                new.get(name),
                3,
                q.whitespace,
            )
        }));
        wire.files.insert(
            0,
            diff::commit_msg_file(revs.old_message(), &revs.revision.message, 3),
        );
        Ok(Json(wire))
    })
    .await
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(super) struct LinesQuery {
    path: String,
    /// The file's name on the old side, when a rename made the two differ —
    /// what `/diff` reported as its `old_path`.
    ///
    /// Both names bound the tree diffs this request takes, and a rename is
    /// paired only when the bound contains both of its ends: named by its new
    /// side alone, a renamed file would come back as a whole-file add.
    old_path: Option<String>,
    /// The old side of the diff: a revision, or the base of revision `n`,
    /// which is also the default.
    against: Option<ChangeVersion>,
    /// How the diff this reveal happens inside compares whitespace.
    ///
    /// A revealed line then has the kind it would have in a hunk.
    #[serde(default)]
    whitespace: Whitespace,
}

/// The whole of file `path`, as diff lines.
///
/// Lets the UI reveal the unchanged runs the shown diff hides. Built from
/// the **same** `old → new` trees and drift tagging as [`revision_diff`],
/// so a revealed line has the exact kind/drift it would have inside a
/// hunk; the client slices the gap it needs. For the synthetic
/// [`diff::COMMIT_MSG_PATH`] and attachment files, the response is their
/// whole text, as [`revision_diff`] compares it.
#[utoipa::path(get, path = "/api/changes/{id}/revisions/{n}/lines", params(("id" = ChangeNumber, Path), ("n" = RevisionNumber, Path), LinesQuery), responses((status = 200, body = FileLines)))]
pub(super) async fn revision_lines(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(ChangeNumber, RevisionNumber)>,
    AppQuery(q): AppQuery<LinesQuery>,
) -> Result<Json<FileLines>, Error> {
    with_conn(state.pool(), move |conn| {
        let change = change_or_404(conn, id)?;
        let revs = resolve_revs(conn, &change, n, q.against)?;
        let wanted = Wanted {
            path: q.path,
            old_path: q.old_path,
        };
        let file = if wanted.path == diff::COMMIT_MSG_PATH {
            Some(diff::commit_msg_file(
                revs.old_message(),
                &revs.revision.message,
                u32::MAX,
            ))
        } else if let Some(name) = diff::attachment_name(&wanted.path) {
            let hash = |version| change.attachment_at(version, name).map(|a| a.hash);
            Some(diff::attachment_file(
                state.blobs(),
                name,
                hash(revs.old_version()).as_ref(),
                hash(ChangeVersion::Revision(n)).as_ref(),
                u32::MAX,
                q.whitespace,
            ))
        } else {
            contained_diff(&revs, u32::MAX, q.whitespace, Some(&wanted))?
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
/// Both names bound the tree diff. Only `path` is rendered.
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
struct Revs {
    git_dir: String,
    revision: RevisionProjection,
    against: Option<RevisionProjection>,
}

impl Revs {
    /// The change version that the old side of the diff shows: the
    /// counterpart, else the base of the revision.
    fn old_version(&self) -> ChangeVersion {
        self.against
            .as_ref()
            .map_or(ChangeVersion::Base(self.revision.number), |a| {
                ChangeVersion::Revision(a.number)
            })
    }

    /// The commit whose tree the old side shows: the counterpart, else the
    /// parent of the revision.
    fn old_commit(&self) -> &Sha {
        self.against
            .as_ref()
            .map_or(&self.revision.parent_sha, |a| &a.commit_sha)
    }

    /// The commit message the old side shows: the counterpart's, else none.
    fn old_message(&self) -> Option<&str> {
        self.against.as_ref().map(|a| a.message.as_str())
    }

    /// The revisions whose trees the diff shows, the selected one first.
    fn revisions(&self) -> impl Iterator<Item = &RevisionProjection> {
        std::iter::once(&self.revision).chain(self.against.as_ref())
    }
}

fn resolve_revs(
    conn: &Connection,
    change: &ChangeProjection,
    n: RevisionNumber,
    against: Option<ChangeVersion>,
) -> Result<Revs, Error> {
    let find = |k: RevisionNumber| {
        change
            .revision(k)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("revision {k} not found")))
    };
    Ok(Revs {
        git_dir: git_dir(conn, change.repo_id)?,
        revision: find(n)?,
        against: match against.unwrap_or(ChangeVersion::Base(n)) {
            ChangeVersion::Base(m) if m == n => None,
            ChangeVersion::Revision(m) => Some(find(m)?),
            base @ ChangeVersion::Base(_) => {
                return Err(Error::bad_request(format!(
                    "the old side of a diff of {} is {} or a revision, not {base}",
                    ChangeVersion::Revision(n),
                    ChangeVersion::Base(n),
                )));
            }
        },
    })
}

/// The wire diff for `revs` with rebase drift contained.
///
/// `parent → commit` of the revision, or `tree(m) → tree(n)` when it has
/// a counterpart to diff against. Owning the choice here is what keeps
/// `/diff` and `/lines` from having to agree on it separately.
///
/// `only` narrows the answer and the git work to one file.
///
/// A plain diff when the two revisions share a parent, and on analysis
/// failure.
fn contained_diff(
    revs: &Revs,
    context: u32,
    whitespace: Whitespace,
    only: Option<&Wanted>,
) -> Result<Diff, Error> {
    let repo = open_repo(&revs.git_dir)?;
    let revision = &revs.revision;
    let new_tree = commit_tree(&repo, &revision.commit_sha)?;
    let old_tree = commit_tree(&repo, revs.old_commit())?;
    let names = only.map(Wanted::names);
    let git = diff::git_diff(&repo, &old_tree, &new_tree, names.as_deref())?;
    let shown = |path: &str| only.is_none_or(|w| w.path == path);
    let plain = || diff::render(&repo, &git, context, whitespace, shown);

    let wire = match revs
        .against
        .as_ref()
        .filter(|a| a.parent_sha != revision.parent_sha)
    {
        None => plain()?,
        Some(m) => rebase::contain(
            &repo,
            &git,
            &at(m),
            &at(revision),
            context,
            whitespace,
            shown,
        )
        .or_else(|e| {
            tracing::warn!("rebase-aware interdiff analysis failed; serving plain diff: {e:#}");
            plain()
        })?,
    };
    Ok(wire)
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(super) struct PortedQuery {
    /// The old side of the diff: a revision, or the base of revision `n`,
    /// which is also the default.
    against: Option<ChangeVersion>,
}

/// `GET /api/changes/{id}/revisions/{n}/ported`.
///
/// Ports every thread of a revision earlier than `n` to `n`, resolved or
/// not, and with `?against=r{m}` to revision `m` as well, `n`'s entries
/// first.
/// Entries for one revision are sorted by thread id.
#[utoipa::path(get, path = "/api/changes/{id}/revisions/{n}/ported", params(("id" = ChangeNumber, Path), ("n" = RevisionNumber, Path), PortedQuery), responses((status = 200, body = Vec<PortedComment>)))]
pub(super) async fn ported_comments(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(ChangeNumber, RevisionNumber)>,
    AppQuery(q): AppQuery<PortedQuery>,
) -> Result<Json<Vec<PortedComment>>, Error> {
    with_conn(state.pool(), move |conn| {
        let change = change_or_404(conn, id)?;
        let revs = resolve_revs(conn, &change, n, q.against)?;
        // Only a thread of an earlier revision can port to either target.
        let latest = revs.revisions().fold(n, |latest, r| latest.max(r.number));
        let threads: Vec<_> = change
            .threads
            .iter()
            .filter(|t| t.anchor.revision() < latest)
            .map(|t| ThreadOrigin {
                thread_id: t.id,
                anchor: t.anchor.clone(),
            })
            .collect();
        let repo = open_repo(&revs.git_dir)?;
        let targets: Vec<_> = revs.revisions().collect();
        Ok(Json(port::port_threads(
            &repo,
            state.blobs(),
            &change,
            &targets,
            &threads,
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
