//! Ported comments: where the threads of earlier revisions land in a
//! revision's trees.
//!
//! The diff between two trees maps an anchor. A file the diff did not
//! touch keeps its anchor, a renamed file keeps it under the new name, and
//! a deleted file demotes it to the change. Inside a file the lines shift
//! by what the edits above them inserted or deleted, and lines an edit
//! rewrote demote to the file (gerrit's `BestPositionOnConflict`). No
//! anchor is ever discarded. The commit message is not in any tree, so its
//! anchors shift through the edits between the two messages instead.
//!
//! The tree diff is bounded to the anchored files, so the cost follows the
//! threads and not the repository; the price is that a rename whose new
//! name no anchor names reads as a deletion.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result};
use git2::{Delta, Repository, Tree};

use nit_types::domain::{
    Anchor, CommentRange, LineAnchor, PortedComment, RevisionProjection, Side, ThreadProjection,
};

use super::diff;
use super::position::{self, Edit, Span};

/// The threads of earlier revisions, ported to `target`.
///
/// Every thread of `threads` written on a revision before `target` comes
/// back at the place its anchor maps to in `target`'s trees, sorted by
/// thread id. `revisions` must hold every revision such a thread names.
///
/// # Errors
///
/// When a revision or tree is missing, or git cannot diff or read.
pub fn port_threads(
    repo: &Repository,
    revisions: &[RevisionProjection],
    target: &RevisionProjection,
    threads: &[ThreadProjection],
) -> Result<Vec<PortedComment>> {
    let mut by_source: BTreeMap<_, Vec<&ThreadProjection>> = BTreeMap::new();
    for thread in threads.iter().filter(|t| t.revision < target.number) {
        by_source.entry(thread.revision).or_default().push(thread);
    }
    let tree =
        |sha| diff::commit_tree(repo, sha).with_context(|| format!("tree for {sha} missing"));
    let dst_of = |side| match side {
        Side::Old => &target.parent_sha,
        Side::New => &target.commit_sha,
    };
    let mut ported = Vec::new();
    for (k, threads) in by_source {
        let source = revisions
            .iter()
            .find(|r| r.number == k)
            .with_context(|| format!("revision {k} missing"))?;
        let (messages, in_trees): (Vec<_>, Vec<_>) = threads
            .into_iter()
            .partition(|t| t.anchor.file() == Some(diff::COMMIT_MSG_PATH));
        if !messages.is_empty() {
            let edits =
                position::buffer_edits(source.message.as_bytes(), target.message.as_bytes());
            for thread in messages {
                ported.push(PortedComment {
                    thread_id: thread.id,
                    revision: target.number,
                    anchor: port_in_file(&thread.anchor, diff::COMMIT_MSG_PATH, Some(&edits)),
                });
            }
        }
        // One tree diff per source tree: the anchors of one revision and
        // side were all written against the same tree.
        for side in [Side::Old, Side::New] {
            let threads: Vec<_> = in_trees
                .iter()
                .filter(|t| t.anchor.side() == side)
                .collect();
            if threads.is_empty() {
                continue;
            }
            let src = tree(match side {
                Side::Old => &source.parent_sha,
                Side::New => &source.commit_sha,
            })?;
            let anchors: Vec<&Anchor> = threads.iter().map(|t| &t.anchor).collect();
            let carried = port_anchors(repo, &src, &tree(dst_of(side))?, &anchors)?;
            ported.extend(
                threads
                    .iter()
                    .zip(carried)
                    .map(|(t, anchor)| PortedComment {
                        thread_id: t.id,
                        revision: target.number,
                        anchor,
                    }),
            );
        }
    }
    ported.sort_by_key(|p| p.thread_id);
    Ok(ported)
}

/// Carries `anchors`, written against `src`, into `dst`.
///
/// Every anchor is returned ported, in the order given.
///
/// # Errors
///
/// When git cannot diff the trees or read a blob.
pub fn port_anchors(
    repo: &Repository,
    src: &Tree<'_>,
    dst: &Tree<'_>,
    anchors: &[&Anchor],
) -> Result<Vec<Anchor>> {
    let unchanged = |anchor: &Anchor| {
        anchor
            .file()
            .map_or(Anchor::Change, |file| port_in_file(anchor, file, Some(&[])))
    };
    if src.id() == dst.id() {
        return Ok(anchors.iter().map(|a| unchanged(a)).collect());
    }
    let files: Vec<String> = anchors
        .iter()
        .filter_map(|anchor| anchor.file())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut deltas: HashMap<String, FileDelta> = HashMap::new();
    if !files.is_empty() {
        for delta in diff::git_diff(repo, src, dst, Some(&files))?.deltas() {
            let Some((old, new)) = diff::delta_names(&delta) else {
                continue;
            };
            let file_delta = match delta.status() {
                Delta::Added => continue,
                Delta::Deleted => FileDelta::Deleted,
                _ => FileDelta::Present {
                    path: new,
                    old_oid: delta.old_file().id(),
                    new_oid: delta.new_file().id(),
                },
            };
            deltas.insert(old, file_delta);
        }
    }
    let mut edits_of: HashMap<&str, Option<Vec<Edit>>> = HashMap::new();
    anchors
        .iter()
        .map(|anchor| {
            let Some(file) = anchor.file() else {
                return Ok(Anchor::Change);
            };
            Ok(match deltas.get(file) {
                None => unchanged(anchor),
                Some(FileDelta::Deleted) => Anchor::Change,
                Some(FileDelta::Present {
                    path,
                    old_oid,
                    new_oid,
                }) => {
                    // Only a line anchor reads the blobs.
                    let edits = if matches!(anchor, Anchor::Line { .. }) {
                        if !edits_of.contains_key(file) {
                            let old = diff::blob_bytes(repo, file, *old_oid)?;
                            let new = diff::blob_bytes(repo, path, *new_oid)?;
                            let edits = old.zip(new).map(|(o, n)| position::buffer_edits(&o, &n));
                            edits_of.insert(file, edits);
                        }
                        edits_of[file].as_deref()
                    } else {
                        Some(&[][..])
                    };
                    port_in_file(anchor, path, edits)
                }
            })
        })
        .collect()
}

/// What the tree diff did to an anchored file.
enum FileDelta {
    Deleted,
    Present {
        path: String,
        old_oid: git2::Oid,
        new_oid: git2::Oid,
    },
}

/// `anchor` under `path`, its lines shifted through `edits`.
///
/// `None` edits mean a binary file, whose lines cannot be mapped. A line
/// an edit rewrote cannot be mapped either. Both give the file anchor.
fn port_in_file(anchor: &Anchor, path: &str, edits: Option<&[Edit]>) -> Anchor {
    let at = match anchor {
        Anchor::Change => return Anchor::Change,
        Anchor::File { .. } => None,
        Anchor::Line { at, .. } => edits.and_then(|edits| port_line(at, edits)),
    };
    Anchor::parse(Some(path.to_owned()), Some(anchor.side()), at).expect("a file is given")
}

/// The anchor's lines through `edits`, or `None` when an edit rewrote them.
///
/// Anchors count lines from 1 and spans from 0.
fn port_line(at: &LineAnchor, edits: &[Edit]) -> Option<LineAnchor> {
    let line = |n: u64| u32::try_from(n).ok();
    let span: Span = match at {
        LineAnchor::Whole(n) => {
            let n = line(*n)?;
            n.checked_sub(1)?..n
        }
        LineAnchor::Selection(r) => line(r.start_line())?.checked_sub(1)?..line(r.end_line())?,
    };
    let moved = position::shift(&span, edits)?;
    let (start, end) = (u64::from(moved.start) + 1, u64::from(moved.end));
    match at {
        LineAnchor::Whole(_) => Some(LineAnchor::Whole(start)),
        LineAnchor::Selection(r) => CommentRange::new(start, r.start_char(), end, r.end_char())
            .ok()
            .map(LineAnchor::Selection),
    }
}
