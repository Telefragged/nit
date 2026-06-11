//! `fixup!`/`squash!` classification and autosquash target attachment.
//!
//! Mirrors git's `todo_list_rearrange_squash` (sequencer.c), the function
//! behind `git rebase -i --autosquash`, with the lookup order documented
//! in docs/data-model.md (scan step 3) — git's actual probe order: among
//! *earlier* commits, the oldest exact-subject match wins, else the
//! needle resolves as a commit-ish, else the oldest subject-prefix match.
//! Fixups of fixups chain to the root target; a fixup with no target is a
//! regular change.

use std::collections::HashMap;

/// How a commit subject classifies in scan step 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixupKind {
    /// `fixup! ` prefix — folded silently.
    Fixup,
    /// `squash! ` prefix — folded like a fixup, but draws a push warning
    /// (its message-editing semantics are interactive).
    Squash,
}

/// Classify a commit subject. Only an exact `fixup! `/`squash! ` prefix
/// (with the space, like git) counts.
pub fn classify(subject: &str) -> Option<FixupKind> {
    if subject.starts_with("fixup! ") {
        Some(FixupKind::Fixup)
    } else if subject.starts_with("squash! ") {
        Some(FixupKind::Squash)
    } else {
        None
    }
}

/// The subject of a commit message: first paragraph, leading blank lines
/// skipped, inner newlines collapsed to spaces (git's
/// `find_commit_subject` + `format_subject`).
pub fn subject_of(message: &str) -> String {
    let body = message.trim_start_matches(['\n', '\r']);
    let para = body.split("\n\n").next().unwrap_or("");
    para.replace('\n', " ").trim().to_string()
}

fn strip_one(s: &str) -> Option<&str> {
    s.strip_prefix("fixup! ")
        .or_else(|| s.strip_prefix("squash! "))
}

/// Strip *all* leading `fixup! `/`squash! ` markers, like git's rearrange
/// loop, returning the needle used for target lookup — or `None` when the
/// subject isn't fixup-ish at all. This is why fixups of fixups attach to
/// the root target.
pub fn fixup_needle(subject: &str) -> Option<&str> {
    let mut p = strip_one(subject)?;
    loop {
        let trimmed = p.trim_start();
        match strip_one(trimmed) {
            Some(rest) => p = rest,
            None => return Some(trimmed),
        }
    }
}

/// A walked commit, oldest first, as seen by the attachment pass.
#[derive(Debug, Clone)]
pub struct CommitMeta {
    /// Full 40-hex sha.
    pub sha: String,
    /// Subject per [`subject_of`].
    pub subject: String,
}

/// Attach fixups to their targets across a walked chain (oldest first).
///
/// Returns, per commit, `Some(root_index)` when the commit is a fixup
/// attached to the regular change at `root_index`, or `None` when it is a
/// regular change (including fixups that found no target).
///
/// `resolve_commitish` resolves a potential commit-ish needle (e.g. an
/// abbreviated sha) to a full sha against the repo; the resolved commit
/// must be an earlier walked commit to count, like git's `commit_todo`
/// check.
pub fn attach_fixups(
    commits: &[CommitMeta],
    resolve_commitish: impl Fn(&str) -> Option<String>,
) -> Vec<Option<usize>> {
    // Full subject -> oldest *non-attached* item, like git's subject2item.
    let mut by_subject: HashMap<&str, usize> = HashMap::new();
    // Sha -> index for every earlier item (git's commit_todo).
    let mut by_sha: HashMap<&str, usize> = HashMap::new();
    let mut roots: Vec<Option<usize>> = vec![None; commits.len()];

    for i in 0..commits.len() {
        let subject = commits[i].subject.as_str();
        let mut target = None;
        if let Some(needle) = fixup_needle(subject) {
            // 1. Oldest exact-subject match (non-attached items only).
            target = by_subject.get(needle).copied();
            // 2. Commit-ish — git tries this before the prefix scan, and
            //    only for space-free needles.
            if target.is_none() && !needle.contains(' ') {
                target =
                    resolve_commitish(needle).and_then(|sha| by_sha.get(sha.as_str()).copied());
            }
            // 3. Oldest subject-prefix match among all earlier commits
            //    (including already-attached fixups; chains to root below).
            if target.is_none() {
                target = (0..i).find(|&j| commits[j].subject.starts_with(needle));
            }
        }
        match target {
            Some(j) => roots[i] = Some(roots[j].unwrap_or(j)),
            None => {
                // Regular change (or fixup with no target): eligible as an
                // exact-subject target for later fixups; oldest wins.
                by_subject.entry(subject).or_insert(i);
            }
        }
        by_sha.insert(commits[i].sha.as_str(), i);
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(pairs: &[(&str, &str)]) -> Vec<CommitMeta> {
        pairs
            .iter()
            .map(|(sha, subject)| CommitMeta {
                sha: (*sha).to_string(),
                subject: (*subject).to_string(),
            })
            .collect()
    }

    fn no_resolve(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn classify_needs_prefix_with_space() {
        assert_eq!(classify("fixup! x"), Some(FixupKind::Fixup));
        assert_eq!(classify("squash! x"), Some(FixupKind::Squash));
        assert_eq!(classify("fixup!"), None);
        assert_eq!(classify("fixup!x"), None);
        assert_eq!(classify("Fixup! x"), None);
        assert_eq!(classify("feat: fixup! x"), None);
    }

    #[test]
    fn subject_extraction() {
        assert_eq!(subject_of("one line\n\nbody"), "one line");
        assert_eq!(subject_of("wrapped\nsubject\n\nbody"), "wrapped subject");
        assert_eq!(subject_of("\n\nleading blank"), "leading blank");
        assert_eq!(subject_of("trailing newline\n"), "trailing newline");
        assert_eq!(subject_of(""), "");
    }

    #[test]
    fn needle_strips_all_markers() {
        assert_eq!(fixup_needle("fixup! add api"), Some("add api"));
        assert_eq!(fixup_needle("squash! add api"), Some("add api"));
        assert_eq!(fixup_needle("fixup! fixup! add api"), Some("add api"));
        assert_eq!(fixup_needle("fixup! squash! fixup! x"), Some("x"));
        assert_eq!(fixup_needle("fixup!  spaced"), Some("spaced"));
        assert_eq!(fixup_needle("not a fixup"), None);
        // A needle that still *contains* the word but not as prefix+space.
        assert_eq!(fixup_needle("fixup! fixup"), Some("fixup"));
    }

    #[test]
    fn exact_subject_match_oldest_wins() {
        let commits = meta(&[
            ("a", "add api"),
            ("b", "add api"), // duplicate subject, newer
            ("c", "fixup! add api"),
        ]);
        assert_eq!(
            attach_fixups(&commits, no_resolve),
            vec![None, None, Some(0)]
        );
    }

    #[test]
    fn prefix_match_when_no_exact() {
        let commits = meta(&[
            ("a", "add api endpoint"),
            ("b", "add api docs"),
            ("c", "fixup! add api"),
        ]);
        // No exact subject "add api"; oldest prefix match is index 0.
        assert_eq!(
            attach_fixups(&commits, no_resolve),
            vec![None, None, Some(0)]
        );
    }

    #[test]
    fn exact_beats_prefix_even_if_newer() {
        let commits = meta(&[
            ("a", "add api endpoint"),
            ("b", "add api"),
            ("c", "fixup! add api"),
        ]);
        assert_eq!(
            attach_fixups(&commits, no_resolve),
            vec![None, None, Some(1)]
        );
    }

    #[test]
    fn commitish_resolution_must_hit_earlier_commit() {
        let sha_a = "aaaa111".to_owned() + &"0".repeat(33);
        let sha_b = "bbbb222".to_owned() + &"0".repeat(33);
        let sha_c = "cccc333".to_owned() + &"0".repeat(33);
        let commits = meta(&[
            (sha_a.as_str(), "add api"),
            (sha_b.as_str(), "fixup! aaaa111"),
            (sha_c.as_str(), "fixup! ffff999"),
        ]);
        let resolve = |needle: &str| -> Option<String> {
            match needle {
                "aaaa111" => Some("aaaa111".to_owned() + &"0".repeat(33)),
                "ffff999" => Some("f".repeat(40)), // resolves, but not in chain
                _ => None,
            }
        };
        assert_eq!(attach_fixups(&commits, resolve), vec![None, Some(0), None]);
    }

    #[test]
    fn fixup_of_fixup_chains_to_root() {
        let commits = meta(&[
            ("a", "add api"),
            ("b", "fixup! add api"),
            ("c", "fixup! fixup! add api"),
        ]);
        assert_eq!(
            attach_fixups(&commits, no_resolve),
            vec![None, Some(0), Some(0)]
        );
    }

    #[test]
    fn fixup_targeting_fixup_by_sha_chains_to_root() {
        let fix_sha = "b".repeat(40);
        let commits = meta(&[
            ("a", "add api"),
            (fix_sha.as_str(), "fixup! add api"),
            ("c", "fixup! bbbbbbb"),
        ]);
        let resolve = |needle: &str| (needle == "bbbbbbb").then(|| "b".repeat(40));
        assert_eq!(
            attach_fixups(&commits, resolve),
            vec![None, Some(0), Some(0)]
        );
    }

    #[test]
    fn untargeted_fixup_is_regular_and_targetable() {
        let commits = meta(&[
            ("a", "fixup! vanished"),        // no target -> regular change
            ("b", "fixup! fixup! vanished"), // exact-matches the *full* subject? no:
            // needle "vanished" misses, prefix scan
            // misses, so this is regular too.
            ("c", "other"),
        ]);
        assert_eq!(attach_fixups(&commits, no_resolve), vec![None, None, None]);
    }

    #[test]
    fn prefix_scan_can_hit_attached_fixup_and_chains_to_root() {
        // Needle "fixup" (no trailing space, so not stripped further) has no
        // exact match; prefix scan hits the attached fixup at index 1 and
        // chains to root 0 — mirrors git scanning all earlier subjects.
        let commits = meta(&[
            ("a", "add api"),
            ("b", "fixup! add api"),
            ("c", "fixup! fixup"),
        ]);
        assert_eq!(
            attach_fixups(&commits, no_resolve),
            vec![None, Some(0), Some(0)]
        );
    }

    #[test]
    fn commitish_beats_prefix_match() {
        // A space-free needle that both resolves as a commit-ish of an
        // earlier commit AND prefix-matches an earlier subject: git probes
        // the commit name before the prefix scan.
        let sha_b = "deadbee".to_owned() + &"0".repeat(33);
        let commits = meta(&[
            ("a", "deadbee cleanup"), // prefix match for the needle
            (sha_b.as_str(), "other work"),
            ("c", "fixup! deadbee"),
        ]);
        let resolve =
            |needle: &str| (needle == "deadbee").then(|| "deadbee".to_owned() + &"0".repeat(33));
        assert_eq!(attach_fixups(&commits, resolve), vec![None, None, Some(1)]);
    }

    #[test]
    fn squash_attaches_like_fixup() {
        let commits = meta(&[("a", "add api"), ("b", "squash! add api")]);
        assert_eq!(attach_fixups(&commits, no_resolve), vec![None, Some(0)]);
    }

    #[test]
    fn fixup_cannot_target_later_commit() {
        let commits = meta(&[("a", "fixup! add api"), ("b", "add api")]);
        assert_eq!(attach_fixups(&commits, no_resolve), vec![None, None]);
    }
}
