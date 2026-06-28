//! Diff JSON rendering and line-text snapshots (docs/api.md "Diff").
//!
//! All functions take trees, not commits: a change's diff is always
//! `parent_sha → commit tree` of the selected revision, an interdiff is
//! `tree(m) → tree(n)` (docs/data-model.md).

use std::path::Path;

use anyhow::{Result, anyhow};
use git2::{Delta, DiffOptions, Patch, Repository, Tree};

use nit_types::diff::{Diff, DiffFile, Hunk, Line};
use nit_types::enums::{FileStatus, LineKind};

/// The reserved synthetic diff path carrying the revision's commit
/// message (docs/api.md "The commit message as a file"). Git tree paths
/// cannot start with `/`, so it can never collide with a real file.
pub const COMMIT_MSG_PATH: &str = "/COMMIT_MSG";

/// The tree of the commit `sha` names, when everything resolves.
#[must_use]
pub fn commit_tree<'r>(repo: &'r Repository, sha: &str) -> Option<Tree<'r>> {
    repo.find_commit(git2::Oid::from_str(sha).ok()?)
        .ok()?
        .tree()
        .ok()
}

/// Render the diff `old → new` as the wire shape: context 3, rename
/// detection, binary files flagged with no hunks.
///
/// # Errors
/// When git can't build or read the diff's patches.
pub fn diff_trees(repo: &Repository, old: &Tree, new: &Tree) -> Result<Diff> {
    diff_trees_ctx(repo, old, new, 3)
}

/// The same diff with every unchanged line kept as context — the source the
/// UI reveals from when expanding a hunk's surroundings (docs/api.md
/// "Expanding context"). Identical classification and drift handling to the
/// shown diff, so revealed lines match it exactly.
///
/// # Errors
/// When git can't build or read the diff's patches.
pub fn diff_trees_full(repo: &Repository, old: &Tree, new: &Tree) -> Result<Diff> {
    diff_trees_ctx(repo, old, new, u32::MAX)
}

fn diff_trees_ctx(repo: &Repository, old: &Tree, new: &Tree, context: u32) -> Result<Diff> {
    let mut opts = DiffOptions::new();
    opts.context_lines(context);
    let mut diff = repo.diff_tree_to_tree(Some(old), Some(new), Some(&mut opts))?;
    let mut find = git2::DiffFindOptions::new();
    find.renames(true);
    diff.find_similar(Some(&mut find))?;

    let mut files = Vec::new();
    for idx in 0..diff.deltas().len() {
        let delta = diff
            .get_delta(idx)
            .ok_or_else(|| anyhow!("diff delta {idx} vanished"))?;
        let Some(status) = delta_status(delta.status()) else {
            continue;
        };
        let path = |f: git2::DiffFile| {
            f.path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let file_path = if delta.status() == Delta::Deleted {
            path(delta.old_file())
        } else {
            path(delta.new_file())
        };
        let old_path = (status == FileStatus::Renamed).then(|| path(delta.old_file()));

        let mut file = DiffFile {
            path: file_path,
            old_path,
            status,
            binary: false,
            additions: 0,
            deletions: 0,
            hunks: Vec::new(),
        };
        match Patch::from_diff(&diff, idx)? {
            Some(mut patch) => {
                if patch.delta().flags().is_binary() {
                    file.binary = true;
                } else {
                    let (_, additions, deletions) = patch.line_stats()?;
                    file.additions = u64::try_from(additions)?;
                    file.deletions = u64::try_from(deletions)?;
                    file.hunks = patch_hunks(&mut patch)?;
                }
            }
            // git2 yields no patch for binary entries in a tree diff.
            None => file.binary = true,
        }
        files.push(file);
    }
    Ok(Diff { files })
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

fn patch_hunks(patch: &mut Patch) -> Result<Vec<Hunk>> {
    let mut hunks = Vec::new();
    for h in 0..patch.num_hunks() {
        let (hunk, _) = patch.hunk(h)?;
        let mut lines = Vec::new();
        for l in 0..patch.num_lines_in_hunk(h)? {
            let line = patch.line_in_hunk(h, l)?;
            let kind = match line.origin() {
                ' ' => LineKind::Context,
                '+' => LineKind::Add,
                '-' => LineKind::Del,
                _ => continue, // eofnl markers etc.
            };
            let text = String::from_utf8_lossy(line.content());
            lines.push(Line {
                kind,
                old: line.old_lineno().map(u64::from),
                new: line.new_lineno().map(u64::from),
                drift: false,
                text: text.strip_suffix('\n').unwrap_or(&text).to_string(),
            });
        }
        hunks.push(Hunk {
            old_start: u64::from(hunk.old_start()),
            old_lines: u64::from(hunk.old_lines()),
            new_start: u64::from(hunk.new_start()),
            new_lines: u64::from(hunk.new_lines()),
            header: hunk_function_context(hunk.header()),
            lines,
        });
    }
    Ok(hunks)
}

/// The synthetic [`COMMIT_MSG_PATH`] entry injected at the front of every
/// diff (docs/api.md "The commit message as a file"): vs parent
/// (`old: None`) the whole message as one all-`add` hunk; interdiff a
/// real line diff `old → new`, identical messages rendered as a single
/// all-`context` hunk so the message stays visible and commentable.
///
/// # Errors
/// When git can't build or read the buffer diff.
pub fn commit_msg_file(old: Option<&str>, new: &str) -> Result<DiffFile> {
    let mut opts = DiffOptions::new();
    opts.context_lines(3);
    let mut patch = Patch::from_buffers(
        old.unwrap_or_default().as_bytes(),
        None,
        new.as_bytes(),
        None,
        Some(&mut opts),
    )?;
    let (_, additions, deletions) = patch.line_stats()?;
    let mut hunks = patch_hunks(&mut patch)?;
    if hunks.is_empty() && !new.is_empty() {
        // Identical interdiff: synthesize the all-context hunk.
        let lines: Vec<Line> = new
            .lines()
            .enumerate()
            .map(|(i, text)| {
                let n = u64::try_from(i)? + 1;
                Ok(Line {
                    kind: LineKind::Context,
                    old: Some(n),
                    new: Some(n),
                    drift: false,
                    text: text.to_string(),
                })
            })
            .collect::<Result<_>>()?;
        let count = u64::try_from(lines.len())?;
        hunks.push(Hunk {
            old_start: 1,
            old_lines: count,
            new_start: 1,
            new_lines: count,
            header: String::new(),
            lines,
        });
    }
    Ok(DiffFile {
        path: COMMIT_MSG_PATH.to_string(),
        old_path: None,
        status: if old.is_some() {
            FileStatus::Modified
        } else {
            FileStatus::Added
        },
        binary: false,
        additions: u64::try_from(additions)?,
        deletions: u64::try_from(deletions)?,
        hunks,
    })
}

/// The function-context part of a raw hunk header:
/// `"@@ -1,5 +1,7 @@ fn main()\n"` → `"fn main()"`.
fn hunk_function_context(header: &[u8]) -> String {
    let s = String::from_utf8_lossy(header);
    match s.splitn(3, "@@").nth(2) {
        Some(rest) => rest.trim().to_string(),
        None => String::new(),
    }
}

/// Line `line` (1-based) of `text`, `None` out of range — the snapshot
/// primitive behind `comments.line_text`, applied to commit messages
/// ([`COMMIT_MSG_PATH`] drafts) and tree files ([`line_text`]) alike.
#[must_use]
pub fn nth_line(text: &str, line: u64) -> Option<String> {
    if line < 1 {
        return None;
    }
    let idx = usize::try_from(line - 1).ok()?;
    text.lines().nth(idx).map(str::to_string)
}

/// The full text of `file` in `tree`, `None` for a missing/binary path —
/// the shared read behind [`line_text`] and [`line_range`].
fn blob_text(repo: &Repository, tree: &Tree, file: &str) -> Option<String> {
    let blob = repo
        .find_blob(tree.get_path(Path::new(file)).ok()?.id())
        .ok()?;
    (!blob.is_binary()).then(|| String::from_utf8_lossy(blob.content()).into_owned())
}

/// Snapshot of line `line` (1-based) of `file` in `tree`, for
/// `comments.line_text`. `None` when the path/line/encoding make that
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

        /// Bytes allow binary content.
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
        let diff = diff_trees(&r.repo, &r.find(t_old), &r.find(t_new)).expect("diff should build");

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
        let diff = diff_trees(&r.repo, &r.find(t_old), &r.find(t_new)).expect("diff should build");

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
        let f = commit_msg_file(None, msg).expect("message file should build");
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
        let f = commit_msg_file(Some(old), new).expect("message file should build");
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
        let f = commit_msg_file(Some(msg), msg).expect("message file should build");
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
    fn hunk_header_function_context() {
        assert_eq!(
            hunk_function_context(b"@@ -1,5 +1,7 @@ fn main()\n"),
            "fn main()"
        );
        assert_eq!(hunk_function_context(b"@@ -1,5 +1,7 @@\n"), "");
        assert_eq!(hunk_function_context(b"garbage"), "");
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

        let shown = diff_trees(&r.repo, &r.find(t_old), &r.find(t_new)).expect("diff builds");
        assert_eq!(shown.files[0].hunks.len(), 2); // a gap the UI would expand

        let full = diff_trees_full(&r.repo, &r.find(t_old), &r.find(t_new)).expect("diff builds");
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
