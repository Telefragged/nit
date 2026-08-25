//! The fold: a **change's** reviewable state is the replay of its log.
//!
//! The log is append-only. [`fold`] applies one wire [`LogEntry`] to a
//! [`ChangeProjection`]; [`replay`] rebuilds a change's projection from its
//! entries; [`change_detail`] publishes one as the wire view. A chain is
//! never folded — it is composed at read time from member projections
//! (`crate::chain`).
//!
//! Pure over `nit_types` alone: no database, no storage serialization, no event
//! publishing. The server's db/storage adapters (`crate::review`) feed it wire
//! `LogEntry`s and store/broadcast the entries it returns; the same code folds
//! the websocket stream client-side once compiled to WebAssembly.
//!
//! Fold-assigned ids: a review's id is its `review` entry's `position` — a log
//! coordinate, reproduced by replay with nothing stored and nothing minted.
//! The change number is the `changes` rowid, carried on the projection.
//! Revision numbers (0-based) are minted **in the fold** by creation order — a
//! pure function of the log, never stored. Thread ids are minted in the fold
//! too: [`fold`] takes an entry by value and, via
//! [`ChangeProjection::mint_thread_id`], fills a new-thread comment's `thread_id` from
//! `next_thread_id` and returns the entry with the id written into its payload,
//! so the caller stores and broadcasts that one value. `next_thread_id` is the
//! single source of truth — the only field minting touches — so a concurrent
//! shared-change push can't duplicate an id, and replay (ids already set) just
//! advances it. The fold therefore requires entries in ascending `position` order.
//!
//! [`ChangeProjection::entries_folded`] is the count of entries consumed (the next
//! `position`): the server stamps it into a projection so a follower resumes folding
//! the live tail at the boundary, and [`fold`] skips any entry below it, so the
//! arm/projection overlap is idempotent, never doubled.

use crate::changes::{ChangeDetail, Review, Revision};
use crate::comments::Thread;
use crate::domain::ChangeId;
use crate::domain::ChangeNumber;
use crate::domain::RevisionNumber;
use crate::domain::{
    Anchor, ChangeProjection, Lifecycle, LifecycleAction, LineAnchor, ReviewProjection,
    RevisionProjection, Side, ThreadComment, ThreadProjection,
};
use crate::domain::{CommentInput, LifecyclePayload, LogEntry, LogPayload, RevisionPayload};

/// Applies one wire entry to a change's projection.
///
/// Mints any new-thread ids into the entry's typed payload and returns
/// the id-bearing entry (the server stores and broadcasts that one).
pub fn fold(change: &mut ChangeProjection, mut entry: LogEntry) -> LogEntry {
    // Idempotent across the projection/live overlap: an entry already folded into
    // this projection (its position below the high-water mark) leaves it untouched,
    // so a follower that re-receives the boundary entries the projection already
    // covers never double-applies them.
    if entry.position < change.entries_folded {
        return entry;
    }
    change.entries_folded = entry.position + 1;
    let now = entry.created_at.clone();
    // A review is identified by where it sits in its change's log: replay
    // reproduces the id with nothing stored and nothing minted, and no reader
    // outside this fold references a review at all.
    let review_id = entry.position;
    match &mut entry.payload {
        LogPayload::Revision(p) => fold_revision(change, p, &now),
        LogPayload::Review(p) => {
            change.reviews.push(ReviewProjection {
                id: review_id,
                revision: p.revision,
                verdict: p.verdict,
                message: p.message.clone(),
                created_at: now.clone(),
            });
            for c in &mut p.comments {
                change.mint_thread_id(c);
                apply_comment(change, c, Some(review_id), &now);
            }
        }
        LogPayload::Comment(c) => {
            change.mint_thread_id(c);
            apply_comment(change, c, None, &now);
        }
        LogPayload::Lifecycle(p) => fold_lifecycle(change, p),
    }
    entry
}

fn fold_revision(change: &mut ChangeProjection, p: &RevisionPayload, now: &str) {
    let number = RevisionNumber::new(
        u64::try_from(change.revisions.len()).expect("revision count fits u64"),
    );
    change.revisions.push(RevisionProjection {
        number,
        commit_sha: p.commit_sha.clone(),
        parent_sha: p.parent_sha.clone(),
        fork_sha: p.fork_sha.clone(),
        message: p.message.clone(),
        resets_status: p.resets_status,
        created_at: now.to_string(),
    });
}

fn fold_lifecycle(change: &mut ChangeProjection, p: &LifecyclePayload) {
    change.lifecycle = match p.action {
        LifecycleAction::Merged => Lifecycle::Merged,
        LifecycleAction::Abandoned => Lifecycle::Abandoned,
        LifecycleAction::Reopened => Lifecycle::Active,
    };
}

/// Applies one comment to a change's threads.
///
/// Its `thread_id` is already resolved by [`ChangeProjection::mint_thread_id`];
/// shared by `review` and `comment`. An unset id is a no-op: the mint left
/// it alone because the body was empty.
fn apply_comment(
    change: &mut ChangeProjection,
    c: &CommentInput,
    review_id: Option<u64>,
    now: &str,
) {
    let Some(tid) = c.thread_id else { return };
    if let Some(thread) = change.threads.iter_mut().find(|t| t.id == tid) {
        if !c.body.trim().is_empty() {
            thread.comments.push(ThreadComment {
                body: c.body.clone(),
                review_id,
                created_at: now.to_string(),
            });
        }
        if let Some(state) = c.resolved {
            thread.resolved = state;
        }
        thread.updated_at = now.to_string();
    } else if !c.body.trim().is_empty() {
        open_thread(change, c, tid, review_id, now);
    }
}

/// Opens a new thread carrying `id` at the comment's anchor.
///
/// `next_thread_id` is kept ahead by [`ChangeProjection::mint_thread_id`], the
/// sole owner of the counter.
fn open_thread(
    change: &mut ChangeProjection,
    c: &CommentInput,
    id: u64,
    review_id: Option<u64>,
    now: &str,
) {
    let revision = c.revision.unwrap_or_else(|| {
        change
            .latest_revision()
            .map_or(RevisionNumber::new(0), |r| r.number)
    });
    change.threads.push(ThreadProjection {
        id,
        revision,
        anchor: c.anchor.clone().unwrap_or(Anchor::Change),
        resolved: c.resolved.unwrap_or(false),
        comments: vec![ThreadComment {
            body: c.body.clone(),
            review_id,
            created_at: now.to_string(),
        }],
        created_at: now.to_string(),
        updated_at: now.to_string(),
    });
}

/// Rebuilds a change's projection from `entries`.
///
/// Requires ascending `position` — `fold()`'s high-water mark silently skips
/// anything out of order.
#[must_use]
pub fn replay(
    id: ChangeNumber,
    repo_id: u64,
    change_id: ChangeId,
    entries: Vec<LogEntry>,
) -> ChangeProjection {
    let mut change = ChangeProjection::new(id, repo_id, change_id);
    for entry in entries {
        fold(&mut change, entry);
    }
    change
}

// Projection → wire: the published view of a change, shared by the
// server's change endpoint and the WebAssembly fold.

#[must_use]
pub fn revision_view(revision: &RevisionProjection) -> Revision {
    Revision {
        number: revision.number,
        commit_sha: revision.commit_sha.clone(),
        parent_sha: revision.parent_sha.clone(),
        fork_sha: revision.fork_sha.clone(),
        message: revision.message.clone(),
        created_at: revision.created_at.clone(),
    }
}

#[must_use]
pub fn review_view(review: &ReviewProjection) -> Review {
    Review {
        id: review.id,
        revision: review.revision,
        verdict: review.verdict,
        message: review.message.clone(),
        created_at: review.created_at.clone(),
    }
}

#[must_use]
pub fn thread_view(t: &ThreadProjection, change_number: ChangeNumber) -> Thread {
    let (file, line, side, range, line_text) = match &t.anchor {
        Anchor::Change => (None, None, Side::New, None, None),
        Anchor::File { file } => (Some(file.clone()), None, Side::New, None, None),
        Anchor::Line {
            file,
            side,
            line_text,
            at,
        } => (
            Some(file.clone()),
            // A selection hangs under the line it ends on.
            Some(match at {
                LineAnchor::Whole(line) => *line,
                LineAnchor::Selection(range) => range.end_line(),
            }),
            *side,
            at.range(),
            line_text.clone(),
        ),
    };
    Thread {
        id: t.id,
        change_number,
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

fn thread_comment_view(c: &ThreadComment) -> crate::comments::ThreadComment {
    crate::comments::ThreadComment {
        body: c.body.clone(),
        review_id: c.review_id,
        created_at: c.created_at.clone(),
    }
}

/// The published projection of a change as the wire [`ChangeDetail`].
///
/// Minus the reviewer's drafts and draft decision: mutable scratch
/// outside the log that the server overlays from the database. The
/// WebAssembly fold returns this verbatim and the browser fills its own
/// drafts in.
#[must_use]
pub fn change_detail(change: &ChangeProjection) -> ChangeDetail {
    ChangeDetail {
        id: change.id,
        repo_id: change.repo_id,
        change_id: change.change_id.clone(),
        revisions: change.revisions.iter().map(revision_view).collect(),
        threads: change
            .threads
            .iter()
            .map(|t| thread_view(t, change.id))
            .collect(),
        drafts: Vec::new(),
        reviews: change.reviews.iter().map(review_view).collect(),
        draft_decision: None,
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::ReviewPayload;
    use crate::domain::{ChangeStatus, LifecycleAction, LineAnchor, Side, Verdict};

    use super::*;
    use crate::tests::{change_id, sha};

    fn empty() -> ChangeProjection {
        ChangeProjection::new(ChangeNumber::new(1), 1, change_id("Iabc"))
    }

    fn entry(position: u64, payload: LogPayload) -> LogEntry {
        LogEntry {
            change_number: ChangeNumber::new(1),
            sequence: position,
            position,
            created_at: format!("t{position}"),
            payload,
        }
    }

    /// A `revision` payload; the fold mints its 0-based number.
    fn revision(name: &str, parent: &str, base: &str, resets: bool) -> LogPayload {
        LogPayload::Revision(RevisionPayload {
            commit_sha: sha(name),
            parent_sha: sha(parent),
            fork_sha: sha(base),
            message: format!("subject {name}\n\nChange-Id: {}\n", change_id("Iabc")),
            resets_status: resets,
        })
    }

    fn review(revision: u64, verdict: Verdict) -> LogPayload {
        let revision = RevisionNumber::new(revision);
        LogPayload::Review(ReviewPayload {
            revision,
            verdict,
            message: "msg".to_string(),
            comments: vec![],
        })
    }

    fn anchored(file: &str, line: u64, body: &str) -> CommentInput {
        CommentInput {
            thread_id: None,
            revision: Some(RevisionNumber::new(0)),
            anchor: Some(Anchor::Line {
                file: file.to_string(),
                side: Side::New,
                line_text: None,
                at: LineAnchor::Whole(line),
            }),
            body: body.to_string(),
            resolved: None,
        }
    }

    fn cinput(thread_id: Option<u64>, body: &str) -> CommentInput {
        CommentInput {
            thread_id,
            revision: None,
            anchor: None,
            body: body.to_string(),
            resolved: None,
        }
    }

    fn folded(payloads: Vec<LogPayload>) -> ChangeProjection {
        let mut c = empty();
        for (i, payload) in payloads.into_iter().enumerate() {
            fold(
                &mut c,
                entry(u64::try_from(i).expect("index fits u64"), payload),
            );
        }
        c
    }

    #[test]
    fn revisions_are_zero_based_and_minted_in_the_fold() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            revision("B", "A", "base", true),
        ]);
        assert_eq!(c.revisions.len(), 2);
        assert_eq!(c.revisions[0].number.get(), 0);
        assert_eq!(c.revisions[1].number.get(), 1);
        assert_eq!(
            c.latest_revision().expect("a revision").commit_sha,
            sha("B")
        );
    }

    #[test]
    fn status_is_per_revision() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::RequestChanges),
            revision("B", "base", "base", true), // reword: new revision
        ]);
        // The review landed on r0; r1 has no review yet and resets → pending.
        assert_eq!(
            c.status_at(RevisionNumber::new(0)),
            ChangeStatus::ChangesRequested
        );
        assert_eq!(c.status_at(RevisionNumber::new(1)), ChangeStatus::Pending);
    }

    #[test]
    fn pure_rebase_carries_status_forward() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
            // r1 is a pure rebase (resets_status = false): inherits r0's approve.
            revision("B", "base2", "base2", false),
        ]);
        assert_eq!(c.status_at(RevisionNumber::new(0)), ChangeStatus::Approved);
        assert_eq!(c.status_at(RevisionNumber::new(1)), ChangeStatus::Approved);
    }

    #[test]
    fn reword_resets_status() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
            revision("B", "base", "base", true),
        ]);
        assert_eq!(c.status_at(RevisionNumber::new(1)), ChangeStatus::Pending);
    }

    #[test]
    fn current_status_tracks_the_latest_revision() {
        assert_eq!(empty().current_status(), ChangeStatus::Pending);
        // current_status is the displayed status at the latest revision: r1 has no
        // review, so pending — even though r0 was approved.
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
            revision("B", "base", "base", true),
        ]);
        assert_eq!(c.status_at(RevisionNumber::new(0)), ChangeStatus::Approved);
        assert_eq!(c.current_status(), ChangeStatus::Pending);
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
        ]);
        assert_eq!(c.current_status(), ChangeStatus::Approved);
        // The lifecycle overlay wins change-wide: abandoned regardless of revision.
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
            LogPayload::lifecycle(LifecycleAction::Abandoned, None, None),
        ]);
        assert_eq!(c.current_status(), ChangeStatus::Abandoned);
    }

    #[test]
    fn merged_paints_at_latest() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
            revision("B", "base", "base", true),
            LogPayload::lifecycle(LifecycleAction::Merged, Some(sha("C")), None),
        ]);
        // Merged shows at the latest revision; older ones keep their own status.
        assert_eq!(c.status_at(RevisionNumber::new(1)), ChangeStatus::Merged);
        assert_eq!(c.status_at(RevisionNumber::new(0)), ChangeStatus::Approved);
        assert!(c.is_terminal());
    }

    #[test]
    fn abandon_then_reopen() {
        let mut c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::RequestChanges),
            LogPayload::lifecycle(LifecycleAction::Abandoned, None, None),
        ]);
        assert_eq!(c.status_at(RevisionNumber::new(0)), ChangeStatus::Abandoned);
        assert!(c.is_terminal());
        // Reopen restores the retained verdict status.
        fold(
            &mut c,
            entry(
                3,
                LogPayload::lifecycle(LifecycleAction::Reopened, None, None),
            ),
        );
        assert!(!c.is_terminal());
        assert_eq!(
            c.status_at(RevisionNumber::new(0)),
            ChangeStatus::ChangesRequested
        );
    }

    #[test]
    fn threads_open_reply_and_resolve() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            LogPayload::Review(ReviewPayload {
                revision: RevisionNumber::new(0),
                verdict: Verdict::Comment,
                message: String::new(),
                comments: vec![anchored("src/x.rs", 3, "look")],
            }),
            LogPayload::Review(ReviewPayload {
                revision: RevisionNumber::new(0),
                verdict: Verdict::Approve,
                message: String::new(),
                comments: vec![CommentInput {
                    thread_id: Some(0),
                    resolved: Some(true),
                    ..cinput(None, "fixed")
                }],
            }),
        ]);
        assert_eq!(c.threads.len(), 1);
        assert_eq!(c.threads[0].comments.len(), 2);
        assert!(c.threads[0].resolved);
    }

    #[test]
    fn agent_comment_opens_a_thread() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            LogPayload::Comment(anchored("a.rs", 1, "why?")),
        ]);
        assert_eq!(c.threads.len(), 1);
        // An author note carries no review_id — that is what marks it author-written.
        assert_eq!(c.threads[0].comments[0].review_id, None);
    }

    #[test]
    fn mint_thread_id_assigns_then_keeps_the_counter_ahead() {
        let mut c = empty();
        let mut open = cinput(None, "opens");
        c.mint_thread_id(&mut open);
        assert_eq!(open.thread_id, Some(0));
        assert_eq!(c.next_thread_id, 1);
        let mut reply = cinput(Some(0), "reply");
        c.mint_thread_id(&mut reply);
        assert_eq!(reply.thread_id, Some(0));
        assert_eq!(c.next_thread_id, 1);
        let mut empty_body = cinput(None, "");
        c.mint_thread_id(&mut empty_body);
        assert_eq!(empty_body.thread_id, None);
        assert_eq!(c.next_thread_id, 1);
        // A stamped id past the counter (a replayed open) pulls it forward.
        let mut stamped = cinput(Some(5), "stamped");
        c.mint_thread_id(&mut stamped);
        assert_eq!(c.next_thread_id, 6);
    }

    #[test]
    fn fold_opens_a_thread_for_a_stamped_unseen_id() {
        let mut c = empty();
        fold(&mut c, entry(0, revision("A", "base", "base", true)));
        fold(
            &mut c,
            entry(
                1,
                LogPayload::Comment(CommentInput {
                    thread_id: Some(3),
                    ..anchored("a.rs", 1, "why?")
                }),
            ),
        );
        assert_eq!(c.threads.len(), 1);
        assert_eq!(c.threads[0].id, 3);
        assert_eq!(c.next_thread_id, 4);
        fold(&mut c, entry(2, LogPayload::Comment(cinput(Some(3), "ok"))));
        assert_eq!(c.threads.len(), 1);
        assert_eq!(c.threads[0].comments.len(), 2);
    }

    #[test]
    fn replay_folds_entries_in_order() {
        let c = replay(
            ChangeNumber::new(1),
            1,
            change_id("Iabc"),
            vec![
                entry(0, revision("A", "base", "base", true)),
                entry(1, review(0, Verdict::Approve)),
            ],
        );
        assert_eq!(c.revisions.len(), 1);
        assert_eq!(c.status_at(RevisionNumber::new(0)), ChangeStatus::Approved);
    }

    #[test]
    fn entries_folded_tracks_the_high_water_mark_and_dedups_the_overlap() {
        let mut c = empty();
        fold(&mut c, entry(0, revision("A", "base", "base", true)));
        fold(&mut c, entry(1, review(0, Verdict::Approve)));
        assert_eq!(c.entries_folded, 2);
        // Re-delivering the projection/live boundary (position 1) is a no-op.
        fold(&mut c, entry(1, review(0, Verdict::RequestChanges)));
        assert_eq!(c.reviews.len(), 1);
        assert_eq!(c.entries_folded, 2);
        assert_eq!(c.status_at(RevisionNumber::new(0)), ChangeStatus::Approved);
        fold(&mut c, entry(2, review(0, Verdict::RequestChanges)));
        assert_eq!(c.reviews.len(), 2);
        assert_eq!(c.entries_folded, 3);
    }
}
