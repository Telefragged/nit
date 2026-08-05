//! Diff JSON rendering and line-text snapshots.
//!
//! All functions take trees, not commits: a change's diff is always
//! `parent_sha → commit tree` of the selected revision, an interdiff is
//! `tree(m) → tree(n)`.

use std::ops::Range;
use std::path::Path;

use anyhow::Result;
use git2::{Delta, Repository, Tree};
use imara_diff::{Algorithm, InternedInput};

use nit_types::diff::{Diff, DiffFile, Hunk, Line};
use nit_types::enums::{FileStatus, LineKind};

/// The reserved synthetic diff path for the revision's commit message.
///
/// Git tree paths cannot start with `/`, so it can never collide with a
/// real file.
pub const COMMIT_MSG_PATH: &str = "/COMMIT_MSG";

/// The tree of the commit `sha` names, when everything resolves.
#[must_use]
pub fn commit_tree<'r>(repo: &'r Repository, sha: &str) -> Option<Tree<'r>> {
    repo.find_commit(git2::Oid::from_str(sha).ok()?)
        .ok()?
        .tree()
        .ok()
}

/// The raw git diff `old → new`, with rename detection.
///
/// The one definition of how nit pairs a delete with an add. Git supplies
/// the deltas only; the lines inside each of them come from `line_hunks`.
/// Built separately from [`render`] so a caller can resolve rebase drift
/// ([`super::rebase::analyze`]) against the same deltas before deciding
/// which are worth rendering.
///
/// # Errors
/// When git can't build the diff or run rename detection.
pub fn git_diff<'r>(
    repo: &'r Repository,
    old: &Tree<'_>,
    new: &Tree<'_>,
) -> Result<git2::Diff<'r>> {
    let mut diff = repo.diff_tree_to_tree(Some(old), Some(new), None)?;
    let mut find = git2::DiffFindOptions::new();
    find.renames(true);
    diff.find_similar(Some(&mut find))?;
    Ok(diff)
}

/// The line diff `old → new`, as the ranges of changed lines on each side.
///
/// Histogram, gerrit's algorithm (`JGit`'s `HistogramDiff`): it anchors on the
/// rarest line the two sides share, so a unique signature pins the alignment
/// where myers — which only minimises the edit script — pairs whatever lies
/// nearest and steals a brace from the neighbouring block. `postprocess_lines`
/// then applies git's indent heuristic to the hunks whose placement is still
/// ambiguous.
pub(super) fn line_edits<T: AsRef<[u8]>>(input: &InternedInput<T>) -> Vec<imara_diff::Hunk> {
    let mut diff = imara_diff::Diff::compute(Algorithm::Histogram, input);
    diff.postprocess_lines(input);
    diff.hunks().collect()
}

/// A delta as the wire carries it.
///
/// Its status, the path it appears under (the new-side name, or the
/// old-side one for a deletion) and the old-side name when a rename made
/// the two differ. `None` for a status the wire never renders, so every
/// walk over the same deltas agrees on which exist.
pub(super) fn delta_file(delta: &git2::DiffDelta) -> Option<(FileStatus, String, Option<String>)> {
    let status = delta_status(delta.status())?;
    let path = |f: git2::DiffFile| {
        f.path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let wire = if delta.status() == Delta::Deleted {
        path(delta.old_file())
    } else {
        path(delta.new_file())
    };
    let old = (status == FileStatus::Renamed).then(|| path(delta.old_file()));
    Some((status, wire, old))
}

/// Renders `diff`'s deltas as the wire shape.
///
/// `context` unchanged lines around each change ([`u32::MAX`] for the
/// full-context `/lines` source).
///
/// `keep` decides which paths are worth rendering — the caller's chance to drop
/// a file (the one it is viewing, or one it has already proved to be pure
/// rebase drift) before its blobs are read and diffed, which is the whole point
/// of taking it.
///
/// # Errors
/// When git can't read a delta's blobs.
pub fn render(
    repo: &Repository,
    diff: &git2::Diff,
    context: u32,
    keep: impl Fn(&str) -> bool,
) -> Result<Diff> {
    let mut files = Vec::new();
    for delta in diff.deltas() {
        let Some((status, path, old_path)) = delta_file(&delta) else {
            continue;
        };
        if !keep(&path) {
            continue;
        }
        // Binary until the blobs prove otherwise, so every unreadable or
        // undiffable file lands in the one place that says so.
        let mut file = DiffFile {
            path,
            old_path,
            status,
            binary: true,
            additions: 0,
            deletions: 0,
            new_total: 0,
            hunks: Vec::new(),
        };
        if let (Some(old), Some(new)) = (
            blob_bytes(repo, &file.path, delta.old_file().id())?,
            blob_bytes(repo, &file.path, delta.new_file().id())?,
        ) {
            let (old, new) = (String::from_utf8_lossy(&old), String::from_utf8_lossy(&new));
            let input = InternedInput::new(&*old, &*new);
            file.binary = false;
            file.new_total = input.after.len() as u64;
            file.hunks = line_hunks(&input, context);
            (file.additions, file.deletions) = stats(&file.hunks);
        }
        files.push(file);
    }
    Ok(Diff { files })
}

/// A blob's bytes, `None` when it is binary.
///
/// The null oid — the absent side of an add or delete — is the empty
/// text.
///
/// `path` decides it as git does, by `.gitattributes` first: a file marked
/// `binary` or `-diff` is binary whatever its bytes look like. Content is only
/// the fallback, and git's own — a NUL byte early in the blob.
///
/// # Errors
/// When git can't read the blob.
pub(super) fn blob_bytes(repo: &Repository, path: &str, oid: git2::Oid) -> Result<Option<Vec<u8>>> {
    if oid.is_zero() {
        return Ok(Some(Vec::new()));
    }
    let attr = repo.get_attr(Path::new(path), "diff", git2::AttrCheckFlags::default());
    if git2::AttrValue::from_string(attr.ok().flatten()) == git2::AttrValue::False {
        return Ok(None);
    }
    let blob = repo.find_blob(oid)?;
    Ok((!blob.is_binary()).then(|| blob.content().to_vec()))
}

fn delta_status(delta: Delta) -> Option<FileStatus> {
    match delta {
        Delta::Added => Some(FileStatus::Added),
        Delta::Deleted => Some(FileStatus::Deleted),
        Delta::Modified | Delta::Typechange => Some(FileStatus::Modified),
        Delta::Renamed | Delta::Copied => Some(FileStatus::Renamed),
        _ => None,
    }
}

/// A file's wire counts.
///
/// Drift lines are the base's work, not the change's, so they never count
/// ([`super::rebase::Drift`] tags them after rendering).
pub(super) fn stats(hunks: &[Hunk]) -> (u64, u64) {
    let (mut additions, mut deletions) = (0, 0);
    for line in hunks.iter().flat_map(|h| &h.lines).filter(|l| !l.drift) {
        match line.kind {
            LineKind::Add => additions += 1,
            LineKind::Del => deletions += 1,
            LineKind::Context => {}
        }
    }
    (additions, deletions)
}

/// The wire hunks of `old → new`.
///
/// Each run of changed lines with `context` unchanged lines on either
/// side, runs closer than twice that merged into one hunk (git's
/// grouping, so a hunk never shows the same line twice).
fn line_hunks(input: &InternedInput<&str>, context: u32) -> Vec<Hunk> {
    let edits: Vec<(Range<usize>, Range<usize>)> = line_edits(input)
        .into_iter()
        .map(|h| (range(h.before), range(h.after)))
        .collect();
    // The tokens carry their line separator; the wire text never does.
    let text = |token| {
        let line: &str = input.interner[token];
        line.strip_suffix('\n').unwrap_or(line)
    };
    let ctx = context as usize;

    // The header is git's default rule: the nearest line above the hunk whose
    // first character is alphabetic, `_` or `$` (no support for the
    // per-language `diff` drivers a `.gitattributes` can name). Hunks ascend,
    // so a cursor that only moves forward reads the file once — searching
    // backwards would re-read it per hunk on one with no declaration at all.
    let (mut scanned, mut header) = (0usize, "");
    let mut header_above = |line: usize| {
        for token in &input.before[scanned..line] {
            let text: &str = input.interner[*token];
            if text.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$') {
                header = text.trim_end();
            }
        }
        scanned = line;
        header.to_string()
    };

    let context_upto = |lines: &mut Vec<Line>, b: &mut usize, a: &mut usize, upto: usize| {
        while *b < upto {
            let line = wire_line(
                LineKind::Context,
                Some(*b),
                Some(*a),
                text(input.before[*b]),
            );
            lines.push(line);
            *b += 1;
            *a += 1;
        }
    };

    edits
        .chunk_by(|a, b| b.0.start - a.0.end <= 2 * ctx)
        .map(|group| {
            let (first, last) = (&group[0], &group[group.len() - 1]);
            // Both sides are identical outside the group, so one reach bounds
            // both: before the first edit their line numbers agree.
            let back = ctx.min(first.0.start);
            let before_end = (last.0.end + ctx).min(input.before.len());
            let (before_start, after_start) = (first.0.start - back, first.1.start - back);
            let (mut b, mut a) = (before_start, after_start);

            let mut lines = Vec::new();
            for (before, after) in group {
                context_upto(&mut lines, &mut b, &mut a, before.start);
                for i in before.clone() {
                    lines.push(wire_line(
                        LineKind::Del,
                        Some(i),
                        None,
                        text(input.before[i]),
                    ));
                }
                for i in after.clone() {
                    lines.push(wire_line(
                        LineKind::Add,
                        None,
                        Some(i),
                        text(input.after[i]),
                    ));
                }
                (b, a) = (before.end, after.end);
            }
            context_upto(&mut lines, &mut b, &mut a, before_end);

            // An empty side reports the line it sits after, not a line it
            // covers, so its start is the 0-based index, not the 1-based
            // number.
            let start = |begin: usize, len: usize| if len == 0 { begin } else { begin + 1 } as u64;
            let (old_lines, new_lines) = (b - before_start, a - after_start);
            Hunk {
                old_start: start(before_start, old_lines),
                old_lines: old_lines as u64,
                new_start: start(after_start, new_lines),
                new_lines: new_lines as u64,
                header: header_above(before_start),
                lines,
            }
        })
        .collect()
}

fn range(r: Range<u32>) -> Range<usize> {
    r.start as usize..r.end as usize
}

fn wire_line(kind: LineKind, old: Option<usize>, new: Option<usize>, text: &str) -> Line {
    let no = |n: Option<usize>| n.map(|n| n as u64 + 1);
    Line {
        kind,
        old: no(old),
        new: no(new),
        drift: false,
        text: text.to_string(),
    }
}

/// The synthetic [`COMMIT_MSG_PATH`] entry at the front of every diff.
///
/// Vs parent (`old: None`) the whole message as one all-`add` hunk;
/// interdiff a real line diff `old → new`, identical messages rendered as
/// a single all-`context` hunk so the message stays visible and
/// commentable.
#[must_use]
pub fn commit_msg_file(old: Option<&str>, new: &str) -> DiffFile {
    let input = InternedInput::new(old.unwrap_or_default(), new);
    let mut hunks = line_hunks(&input, 3);
    let (additions, deletions) = stats(&hunks);
    if hunks.is_empty() && !new.is_empty() {
        let lines: Vec<Line> = new
            .lines()
            .enumerate()
            .map(|(i, text)| wire_line(LineKind::Context, Some(i), Some(i), text))
            .collect();
        let count = lines.len() as u64;
        hunks.push(Hunk {
            old_start: 1,
            old_lines: count,
            new_start: 1,
            new_lines: count,
            header: String::new(),
            lines,
        });
    }
    DiffFile {
        path: COMMIT_MSG_PATH.to_string(),
        old_path: None,
        status: if old.is_some() {
            FileStatus::Modified
        } else {
            FileStatus::Added
        },
        binary: false,
        additions,
        deletions,
        new_total: input.after.len() as u64,
        hunks,
    }
}

/// Line `line` (1-based) of `text`, `None` out of range.
///
/// The snapshot primitive behind `comments.line_text`, applied to commit
/// messages ([`COMMIT_MSG_PATH`] drafts) and tree files ([`line_text`])
/// alike.
#[must_use]
pub fn nth_line(text: &str, line: u64) -> Option<String> {
    if line < 1 {
        return None;
    }
    let idx = usize::try_from(line - 1).ok()?;
    text.lines().nth(idx).map(str::to_string)
}

/// The full text of `file` in `tree`, `None` for a missing/binary path.
///
/// The shared read behind [`line_text`] and [`line_range`].
fn blob_text(repo: &Repository, tree: &Tree, file: &str) -> Option<String> {
    let oid = tree.get_path(Path::new(file)).ok()?.id();
    let bytes = blob_bytes(repo, file, oid).ok()??;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Snapshot of line `line` (1-based) of `file` in `tree`.
///
/// For `comments.line_text`; `None` when the path/line/encoding make that
/// impossible.
#[must_use]
pub fn line_text(repo: &Repository, tree: &Tree, file: &str, line: u64) -> Option<String> {
    nth_line(&blob_text(repo, tree, file)?, line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::RepositoryInitOptions;

    struct Repo {
        _dir: tempfile::TempDir,
        repo: Repository,
    }

    impl Repo {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir should create");
            let mut opts = RepositoryInitOptions::new();
            opts.initial_head("refs/heads/main");
            let repo =
                Repository::init_opts(dir.path().join("r"), &opts).expect("test repo should init");
            Repo { _dir: dir, repo }
        }

        fn tree(&self, files: &[(&str, &[u8])]) -> git2::Oid {
            let mut builder = self
                .repo
                .treebuilder(None)
                .expect("treebuilder should create");
            for (path, content) in files {
                let blob = self.repo.blob(content).expect("blob should write");
                builder
                    .insert(path, blob, 0o100_644)
                    .expect("tree entry should insert");
            }
            builder.write().expect("tree should write")
        }

        fn find(&self, oid: git2::Oid) -> Tree<'_> {
            self.repo.find_tree(oid).expect("tree should exist")
        }

        fn write_attributes(&self, rules: &str) {
            let workdir = self.repo.workdir().expect("test repo has a workdir");
            std::fs::write(workdir.join(".gitattributes"), rules)
                .expect(".gitattributes should write");
        }
    }

    fn shown(repo: &Repository, old: &Tree, new: &Tree) -> Diff {
        let diff = git_diff(repo, old, new).expect("diff builds");
        render(repo, &diff, 3, |_| true).expect("diff renders")
    }

    /// One file's diff with every unchanged line kept as context — what the UI
    /// reveals from when expanding a hunk's surroundings.
    fn full(repo: &Repository, old: &Tree, new: &Tree, only: &str) -> Diff {
        let diff = git_diff(repo, old, new).expect("diff builds");
        render(repo, &diff, u32::MAX, |p| p == only).expect("diff renders")
    }

    fn lines(n: std::ops::RangeInclusive<u64>) -> String {
        use std::fmt::Write;
        n.fold(String::new(), |mut s, i| {
            writeln!(s, "line {i}").expect("write to String is infallible");
            s
        })
    }

    #[test]
    fn modified_file_hunks_and_line_numbers() {
        let r = Repo::new();
        let old = lines(1..=20);
        let new = old
            .replace("line 3\n", "line three\n")
            .replace("line 17\n", "line 17\nline 17.5\n");
        let t_old = r.tree(&[("a.txt", old.as_bytes())]);
        let t_new = r.tree(&[("a.txt", new.as_bytes())]);
        let diff = shown(&r.repo, &r.find(t_old), &r.find(t_new));

        assert_eq!(diff.files.len(), 1);
        let f = &diff.files[0];
        assert_eq!(f.path, "a.txt");
        assert_eq!(f.old_path, None);
        assert_eq!(f.status, FileStatus::Modified);
        assert!(!f.binary);
        assert_eq!((f.additions, f.deletions), (2, 1));
        assert_eq!(f.hunks.len(), 2);

        let h0 = &f.hunks[0];
        assert_eq!(
            (h0.old_start, h0.old_lines, h0.new_start, h0.new_lines),
            (1, 6, 1, 6)
        );
        let del = h0
            .lines
            .iter()
            .find(|l| l.kind == LineKind::Del)
            .expect("del line should exist");
        assert_eq!(
            (del.old, del.new, del.text.as_str()),
            (Some(3), None, "line 3")
        );
        let add = h0
            .lines
            .iter()
            .find(|l| l.kind == LineKind::Add)
            .expect("add line should exist");
        assert_eq!(
            (add.old, add.new, add.text.as_str()),
            (None, Some(3), "line three")
        );
        let ctx = &h0.lines[0];
        assert_eq!(
            (ctx.kind, ctx.old, ctx.new),
            (LineKind::Context, Some(1), Some(1))
        );

        let h1 = &f.hunks[1];
        assert_eq!(h1.old_start, 15); // 3 context lines above the insertion
        let add = h1
            .lines
            .iter()
            .find(|l| l.kind == LineKind::Add)
            .expect("add line should exist");
        assert_eq!((add.new, add.text.as_str()), (Some(18), "line 17.5"));
    }

    #[test]
    fn gitattributes_marks_a_text_file_binary() {
        let r = Repo::new();
        r.write_attributes("*.bin -diff\n");
        let t_old = r.tree(&[("data.bin", b"one\ntwo\n".as_slice())]);
        let t_new = r.tree(&[("data.bin", b"one\nTWO\n".as_slice())]);
        let diff = shown(&r.repo, &r.find(t_old), &r.find(t_new));

        // Plain text by content, but the repo says not to diff it — as git
        // does, the attribute wins and no lines are rendered.
        let f = &diff.files[0];
        assert!(f.binary);
        assert!(f.hunks.is_empty());
        assert_eq!((f.additions, f.deletions), (0, 0));
    }

    #[test]
    fn ambiguous_insertion_lands_on_the_block_boundary() {
        let r = Repo::new();
        let old = "outer = [\n  {\n    a: 1,\n  },\n  {\n    b: 2,\n  },\n]\n";
        let new =
            "outer = [\n  {\n    a: 1,\n  },\n  {\n    c: 3,\n  },\n  {\n    b: 2,\n  },\n]\n";
        let t_old = r.tree(&[("a.js", old.as_bytes())]);
        let t_new = r.tree(&[("a.js", new.as_bytes())]);
        let diff = shown(&r.repo, &r.find(t_old), &r.find(t_new));

        // Every shift of this insertion costs the same three lines, so only
        // the indent heuristic decides where it lands: the whole inserted
        // object, not a tail of one object plus the head of the next.
        let added: Vec<&str> = diff.files[0].hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Add)
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(added, vec!["  {", "    c: 3,", "  },"]);
    }

    #[test]
    fn added_deleted_renamed_binary() {
        let r = Repo::new();
        let keep = lines(1..=30);
        let renamed_body = lines(1..=40);
        let renamed_tweaked = renamed_body.replace("line 40\n", "line forty\n");
        let t_old = r.tree(&[
            ("doomed.txt", b"bye\n".as_slice()),
            ("keep.txt", keep.as_bytes()),
            ("old_name.txt", renamed_body.as_bytes()),
        ]);
        let t_new = r.tree(&[
            ("bin.dat", b"\x00\x01\x02\xff".as_slice()),
            ("fresh.txt", b"hi\n".as_slice()),
            ("keep.txt", keep.as_bytes()),
            ("new_name.txt", renamed_tweaked.as_bytes()),
        ]);
        let diff = shown(&r.repo, &r.find(t_old), &r.find(t_new));

        let by_path = |p: &str| {
            diff.files
                .iter()
                .find(|f| f.path == p)
                .expect("file should be in the diff")
        };
        assert_eq!(diff.files.len(), 4); // keep.txt untouched

        let added = by_path("fresh.txt");
        assert_eq!(added.status, FileStatus::Added);
        assert_eq!((added.additions, added.deletions), (1, 0));
        let l = &added.hunks[0].lines[0];
        assert_eq!((l.kind, l.old, l.new), (LineKind::Add, None, Some(1)));

        let deleted = by_path("doomed.txt");
        assert_eq!(deleted.status, FileStatus::Deleted);
        assert_eq!((deleted.additions, deleted.deletions), (0, 1));

        let renamed = by_path("new_name.txt");
        assert_eq!(renamed.status, FileStatus::Renamed);
        assert_eq!(renamed.old_path.as_deref(), Some("old_name.txt"));

        let bin = by_path("bin.dat");
        assert!(bin.binary);
        assert!(bin.hunks.is_empty());
        assert_eq!((bin.additions, bin.deletions), (0, 0));
    }

    #[test]
    fn commit_msg_file_vs_parent_is_all_add() {
        let msg = "feat: subject\n\nA body line.\n\nChange-Id: Iabc\n";
        let f = commit_msg_file(None, msg);
        assert_eq!(f.path, COMMIT_MSG_PATH);
        assert_eq!(f.old_path, None);
        assert_eq!(f.status, FileStatus::Added);
        assert!(!f.binary);
        assert_eq!((f.additions, f.deletions), (5, 0));
        assert_eq!(f.hunks.len(), 1);
        let h = &f.hunks[0];
        assert_eq!(
            (h.old_start, h.old_lines, h.new_start, h.new_lines),
            (0, 0, 1, 5)
        );
        let texts: Vec<(LineKind, Option<u64>, Option<u64>, &str)> = h
            .lines
            .iter()
            .map(|l| (l.kind, l.old, l.new, l.text.as_str()))
            .collect();
        assert_eq!(
            texts,
            vec![
                (LineKind::Add, None, Some(1), "feat: subject"),
                (LineKind::Add, None, Some(2), ""),
                (LineKind::Add, None, Some(3), "A body line."),
                (LineKind::Add, None, Some(4), ""),
                (LineKind::Add, None, Some(5), "Change-Id: Iabc"),
            ]
        );
    }

    #[test]
    fn commit_msg_file_interdiff_diffs_messages() {
        let old = "feat: subject\n\nOld body.\n\nChange-Id: Iabc\n";
        let new = "feat: subject\n\nNew body,\nover two lines.\n\nChange-Id: Iabc\n";
        let f = commit_msg_file(Some(old), new);
        assert_eq!(f.path, COMMIT_MSG_PATH);
        assert_eq!(f.status, FileStatus::Modified);
        assert_eq!((f.additions, f.deletions), (2, 1));
        assert_eq!(f.hunks.len(), 1);
        let del = f.hunks[0]
            .lines
            .iter()
            .find(|l| l.kind == LineKind::Del)
            .expect("del line should exist");
        assert_eq!((del.old, del.text.as_str()), (Some(3), "Old body."));
        let adds: Vec<(&str, Option<u64>)> = f.hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Add)
            .map(|l| (l.text.as_str(), l.new))
            .collect();
        assert_eq!(
            adds,
            vec![("New body,", Some(3)), ("over two lines.", Some(4))]
        );
    }

    #[test]
    fn commit_msg_file_identical_interdiff_is_all_context() {
        let msg = "feat: subject\n\nSame body.\n\nChange-Id: Iabc\n";
        let f = commit_msg_file(Some(msg), msg);
        assert_eq!(f.status, FileStatus::Modified);
        assert_eq!((f.additions, f.deletions), (0, 0));
        assert_eq!(f.hunks.len(), 1);
        let h = &f.hunks[0];
        assert_eq!(
            (h.old_start, h.old_lines, h.new_start, h.new_lines),
            (1, 5, 1, 5)
        );
        assert_eq!(h.header, "");
        assert!(h.lines.iter().all(|l| l.kind == LineKind::Context));
        assert_eq!(h.lines.len(), 5);
        let l = &h.lines[4];
        assert_eq!(
            (l.old, l.new, l.text.as_str()),
            (Some(5), Some(5), "Change-Id: Iabc")
        );
    }

    #[test]
    fn hunk_header_names_the_enclosing_declaration() {
        let old = "fn main() {\n    a();\n    b();\n    c();\n    d();\n}\n";
        let new = old.replace("    d();\n", "    D();\n");
        let r = Repo::new();
        let t_old = r.tree(&[("a.rs", old.as_bytes())]);
        let t_new = r.tree(&[("a.rs", new.as_bytes())]);
        let diff = shown(&r.repo, &r.find(t_old), &r.find(t_new));
        assert_eq!(diff.files[0].hunks[0].header, "fn main() {");

        // Nothing above the hunk starts a declaration: no header.
        let old = "    a();\n    b();\n";
        let new = "    a();\n    B();\n";
        let t_old = r.tree(&[("b.rs", old.as_bytes())]);
        let t_new = r.tree(&[("b.rs", new.as_bytes())]);
        let diff = shown(&r.repo, &r.find(t_old), &r.find(t_new));
        assert_eq!(diff.files[0].hunks[0].header, "");
    }

    #[test]
    fn nth_line_snapshot() {
        let msg = "subject\n\nbody\n";
        assert_eq!(nth_line(msg, 1).as_deref(), Some("subject"));
        assert_eq!(nth_line(msg, 2).as_deref(), Some(""));
        assert_eq!(nth_line(msg, 3).as_deref(), Some("body"));
        assert_eq!(nth_line(msg, 4), None);
        assert_eq!(nth_line(msg, 0), None);
    }

    #[test]
    fn line_text_snapshot() {
        let r = Repo::new();
        let tree = r.find(r.tree(&[
            ("a.txt", b"first\nsecond\n".as_slice()),
            ("bin.dat", b"\x00\x01".as_slice()),
        ]));
        assert_eq!(
            line_text(&r.repo, &tree, "a.txt", 2).as_deref(),
            Some("second")
        );
        assert_eq!(line_text(&r.repo, &tree, "a.txt", 3), None);
        assert_eq!(line_text(&r.repo, &tree, "a.txt", 0), None);
        assert_eq!(line_text(&r.repo, &tree, "missing.txt", 1), None);
        assert_eq!(line_text(&r.repo, &tree, "bin.dat", 1), None);
    }

    #[test]
    fn diff_trees_full_keeps_every_unchanged_line() {
        let r = Repo::new();
        let old = lines(1..=20);
        // Edits far apart: the shown diff splits into two hunks, full context
        // keeps them in one run with every unchanged line present.
        let new = old
            .replace("line 3\n", "line three\n")
            .replace("line 18\n", "line eighteen\n");
        let t_old = r.tree(&[("a.txt", old.as_bytes())]);
        let t_new = r.tree(&[("a.txt", new.as_bytes())]);

        let shown = shown(&r.repo, &r.find(t_old), &r.find(t_new));
        assert_eq!(shown.files[0].hunks.len(), 2); // a gap the UI would expand

        let full = full(&r.repo, &r.find(t_old), &r.find(t_new), "a.txt");
        assert_eq!(full.files.len(), 1); // bounded to the requested file
        let f = &full.files[0];
        assert_eq!(f.hunks.len(), 1); // one run, no gap
        let lines = &f.hunks[0].lines;
        // 20 originals minus 2 replaced plus 2 replacements = 22 wire lines.
        assert_eq!(lines.len(), 22);
        // The lines the shown diff hid (e.g. new line 10) are present here.
        let ten = lines
            .iter()
            .find(|l| l.new == Some(10))
            .expect("the gap's line 10 is kept");
        assert_eq!(
            (ten.kind, ten.text.as_str()),
            (LineKind::Context, "line 10")
        );
    }
}
