//! View assembly: the per-change folds (`crate::review`) + chain derivation
//! (`crate::chain`) + reviewer drafts → the wire shapes of docs/api.md. Chain
//! views take a [`RepoView`] snapshot plus the repo handle (for query-time tip
//! names); draft rows come from the database.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use git2::Repository;
use rusqlite::Connection;

use crate::chain::{self, PathMember, RepoView};
use crate::db;
use crate::enums::{ChangeStatus, Side};
use crate::gitscan::{self, identity::subject_of};
use crate::review::{self, Anchor, ChangeProj, Entry, ThreadComment, ThreadProj};

use super::{Error, types};

// ---------------------------------------------------------------------------
// Chains (derived)

/// Build the derived `Chain` for one tip commit-sha — the dashboard list
/// entry, the chain page, and the push result all share this one shape.
#[must_use]
pub fn build_chain(view: &RepoView, repo_id: u64, tip_sha: &str) -> types::Chain {
    let path = view.path_from_tip(tip_sha);
    let tip_change_id = path.last().map_or(0, |m| m.change_id);
    types::Chain {
        tip_change_id,
        repo_id,
        state: chain::derive_state(view, &path),
        partial: chain::is_partial(view, &path),
        path: path_entries(view, &path),
    }
}

/// One `PathEntry` per member, read at the revision the path pins.
fn path_entries(view: &RepoView, path: &[PathMember]) -> Vec<types::PathEntry> {
    path.iter()
        .enumerate()
        .filter_map(|(position, m)| {
            view.change(m.change_id)
                .map(|c| path_entry(c, m, u64::try_from(position).unwrap_or(u64::MAX)))
        })
        .collect()
}

fn path_entry(change: &ChangeProj, member: &PathMember, position: u64) -> types::PathEntry {
    let revision = member.revision;
    let subject = change
        .revision(revision)
        .map(|r| subject_of(&r.message))
        .unwrap_or_default();
    types::PathEntry {
        change_id: change.id,
        position,
        change_key: change.change_key.clone(),
        revision,
        status: change.status_at(revision),
        subject,
        commit_sha: member.commit_sha.clone(),
    }
}

// ---------------------------------------------------------------------------
// Graph (the spine-centered DAG; docs/api.md "Graph")

/// Assemble the repo's change graph: the canonical HEAD anchor and a
/// `merged_window` of merged history below it (a git walk), plus every active
/// change ascending above it (the same derivation as a chain, unioned and
/// deduplicated by commit-sha). Nodes are returned in topological row order.
///
/// # Errors
/// When the canonical branch can't be walked.
pub fn build_graph(
    repo: &Repository,
    view: &RepoView,
    repo_id: u64,
    base_ref: &str,
    merged_window: u64,
) -> Result<types::RepoGraph> {
    let (history, history_truncated) =
        gitscan::canonical_history(repo, base_ref, merged_window).map_err(anyhow::Error::msg)?;
    let anchor = history.first().map_or_else(String::new, |h| h.sha.clone());

    let mut nodes: Vec<types::GraphNode> = Vec::new();
    let mut shas: HashSet<String> = HashSet::new();

    // History region: the HEAD anchor (depth 0) + merged commits below it.
    for (depth, h) in history.iter().enumerate() {
        let change = h.change_key.as_deref().and_then(|k| view.change_by_key(k));
        nodes.push(types::GraphNode {
            commit_sha: h.sha.clone(),
            section: if depth == 0 {
                types::GraphSection::Head
            } else {
                types::GraphSection::History
            },
            subject: h.subject.clone(),
            status: ChangeStatus::Merged,
            parents: h.parents.clone(),
            // change_id/change_key are coupled (docs/api.md): both come from the
            // matched change, so a bare commit (a foreign/pre-nit Change-Id
            // trailer with no change) reports both null, not an orphan key.
            change_id: change.map(|c| c.id),
            change_key: change.map(|c| c.change_key.clone()),
            revision: None,
        });
        shas.insert(h.sha.clone());
    }

    // Open region: active changes ascending, deduplicated by commit-sha.
    for node in view.open_nodes() {
        if !shas.insert(node.commit_sha.clone()) {
            continue; // already placed (an anchor/history sha)
        }
        let Some(change) = view.change(node.change_id) else {
            continue;
        };
        let subject = change
            .revision(node.revision)
            .map(|r| subject_of(&r.message))
            .unwrap_or_default();
        nodes.push(types::GraphNode {
            commit_sha: node.commit_sha,
            section: types::GraphSection::Open,
            subject,
            status: change.status_at(node.revision),
            parents: vec![node.parent_sha],
            change_id: Some(change.id),
            change_key: Some(change.change_key.clone()),
            revision: Some(node.revision),
        });
    }

    // An open chain's root keeps its real fork (`base_sha`): the client draws a
    // "behind" edge to it when it is a visible history node, or dangles it into
    // the "earlier history hidden" marker when the fork predates the window.

    // Row order: the open region ascends above the canonical HEAD, so order it
    // topologically among itself (children before parents); the HEAD anchor and
    // its merged history keep the canonical-branch walk order below it. A single
    // global topo would let HEAD — a leaf when nothing is built on it — float to
    // the top, which is wrong whenever the whole chain forks behind HEAD.
    let open_shas: HashSet<&str> = nodes
        .iter()
        .filter(|n| n.section == types::GraphSection::Open)
        .map(|n| n.commit_sha.as_str())
        .collect();
    let open_pairs: Vec<(String, Vec<String>)> = nodes
        .iter()
        .filter(|n| n.section == types::GraphSection::Open)
        .map(|n| {
            let parents = n
                .parents
                .iter()
                .filter(|p| open_shas.contains(p.as_str()))
                .cloned()
                .collect();
            (n.commit_sha.clone(), parents)
        })
        .collect();
    let open_pos: HashMap<String, usize> = chain::graph_row_order(&open_pairs)
        .into_iter()
        .enumerate()
        .map(|(i, sha)| (sha, i))
        .collect();
    let (mut open_nodes, rest): (Vec<_>, Vec<_>) = nodes
        .into_iter()
        .partition(|n| n.section == types::GraphSection::Open);
    open_nodes.sort_by_key(|n| open_pos.get(&n.commit_sha).copied().unwrap_or(usize::MAX));
    open_nodes.extend(rest); // the HEAD anchor + history, in canonical-walk order
    let nodes = open_nodes;

    Ok(types::RepoGraph {
        repo_id,
        anchor,
        history_truncated,
        nodes,
    })
}

/// The tip whose path walks `change` at `revision`, else the change's own
/// revision sha (a dangling change is its own degenerate tip). Enumerates
/// abandoned leaves too (membership-inert), so an abandoned change resolves to
/// a real chain, not only the degenerate fallback.
#[must_use]
pub fn tip_for(view: &RepoView, change_id: u64, revision: u64) -> Option<String> {
    for tip in view.enumerable_tips() {
        let path = view.path_from_tip(&tip);
        if path
            .iter()
            .any(|m| m.change_id == change_id && m.revision == revision)
        {
            return Some(tip);
        }
    }
    view.change(change_id)
        .and_then(|c| c.revision(revision))
        .map(|r| r.commit_sha.clone())
}

/// Resolve the `(revision, tip_sha)` a chain handler operates on: the
/// explicitly `requested` revision, else the change's latest. The path-walking
/// tip is found via [`tip_for`].
///
/// # Errors
/// 404 if the change has no revisions, or if `requested` names a revision with
/// no enclosing tip.
pub fn resolve_revision_tip(
    view: &RepoView,
    change_id: u64,
    requested: Option<u64>,
) -> Result<(u64, String), Error> {
    let revision = requested
        .or_else(|| {
            view.change(change_id)
                .and_then(|c| c.latest_revision().map(|r| r.number))
        })
        .ok_or_else(|| Error::not_found(format!("change {change_id} has no revisions")))?;
    let tip_sha = tip_for(view, change_id, revision)
        .ok_or_else(|| Error::not_found(format!("revision {revision} not found")))?;
    Ok((revision, tip_sha))
}

// ---------------------------------------------------------------------------
// Threads + drafts

/// A published thread → its wire shape, projecting its [`Anchor`] back to the
/// flat `file`/`line`/`side`/`range`/`line_text` fields.
#[must_use]
pub fn thread_view(t: &ThreadProj, change_id: u64) -> types::Thread {
    let (file, line, side, range, line_text) = match &t.anchor {
        Anchor::Change => (None, None, Side::New, None, None),
        Anchor::File { file } => (Some(file.clone()), None, Side::New, None, None),
        Anchor::Line {
            file,
            side,
            line,
            line_text,
            range,
        } => (
            Some(file.clone()),
            Some(*line),
            *side,
            *range,
            line_text.clone(),
        ),
    };
    types::Thread {
        id: t.id,
        change_id,
        revision: t.revision,
        file,
        line,
        side,
        range,
        line_text,
        resolved: t.resolved,
        comments: t.comments.iter().map(thread_comment_view).collect(),
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
    }
}

#[must_use]
fn thread_comment_view(c: &ThreadComment) -> types::ThreadComment {
    types::ThreadComment {
        body: c.body.clone(),
        review_id: c.review_id,
        created_at: c.created_at.clone(),
    }
}

/// A draft row → its wire shape.
#[must_use]
pub fn draft_view(d: &db::DraftRow, change_id: u64) -> types::Draft {
    types::Draft {
        id: d.id,
        change_id,
        thread_id: d.thread_id,
        revision: d.revision,
        file: d.file.clone(),
        line: d.line,
        side: d.side,
        range: d.range,
        line_text: d.line_text.clone(),
        body: d.body.clone(),
        resolved: d.resolved.unwrap_or(false),
        created_at: d.created_at.clone(),
        updated_at: d.updated_at.clone(),
    }
}

// ---------------------------------------------------------------------------
// Change detail

/// Change detail JSON for **one** change: every revision, every published
/// thread, the reviewer's open drafts, every review, and the staged decision.
/// A pure read of the single fold — the chains a change sits on come from the
/// chain endpoints (`GET /api/chains/{id}`), so a change read builds no view.
///
/// # Errors
/// When reading drafts fails.
pub fn build_change_detail(conn: &Connection, change: &ChangeProj) -> Result<types::ChangeDetail> {
    let revisions: Vec<types::Revision> = change.revisions.iter().map(revision_json).collect();
    let threads: Vec<types::Thread> = change
        .threads
        .iter()
        .map(|t| thread_view(t, change.id))
        .collect();
    let drafts: Vec<types::Draft> = db::drafts_for_change(conn, change.id)?
        .iter()
        .map(|d| draft_view(d, change.id))
        .collect();
    let reviews = change.reviews.iter().map(review_json).collect();
    let draft_decision = db::get_draft_review(conn, change.id)?.map(|r| types::StagedDecision {
        decision: r.decision,
        message: r.message,
    });
    Ok(types::ChangeDetail {
        id: change.id,
        repo_id: change.repo_id,
        change_key: change.change_key.clone(),
        revisions,
        threads,
        drafts,
        reviews,
        draft_decision,
    })
}

#[must_use]
pub fn revision_json(rev: &review::RevisionProj) -> types::Revision {
    types::Revision {
        number: rev.number,
        commit_sha: rev.commit_sha.clone(),
        parent_sha: rev.parent_sha.clone(),
        base_sha: rev.base_sha.clone(),
        partial: rev.partial,
        message: rev.message.clone(),
        created_at: rev.created_at.clone(),
    }
}

#[must_use]
pub fn review_json(review: &review::ReviewProj) -> types::Review {
    types::Review {
        id: review.id,
        revision: review.revision,
        verdict: review.verdict,
        message: review.message.clone(),
        created_at: review.created_at.clone(),
    }
}

// ---------------------------------------------------------------------------
// Log entries

/// A folded log entry → its wire shape, serializing the typed payload to JSON
/// at this boundary.
///
/// # Panics
/// If the payload fails to serialize — impossible for these plain structs.
#[must_use]
pub fn log_entry_view(change_id: u64, entry: &Entry) -> types::LogEntry {
    types::LogEntry {
        change_id,
        idx: entry.idx,
        seq: entry.seq,
        kind: entry.kind(),
        created_at: entry.created_at.clone(),
        payload: entry
            .payload
            .to_value()
            .expect("log entry payload serializes"),
    }
}
