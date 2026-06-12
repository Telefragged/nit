//! Diff JSON rendering and comment-anchor porting (docs/api.md "Diff" and
//! "Comment rendering across revisions").
//!
//! All functions take trees, not commits: a change's diff is always
//! `parent_sha → effective_tree` of the selected revision, an interdiff is
//! `effective_tree(m) → effective_tree(n)` (docs/data-model.md).

use std::path::Path;

use anyhow::{Result, anyhow};
use git2::{Delta, DiffOptions, Patch, Repository, Tree};

use super::types;

/// Render the diff `old → new` as the wire shape: context 3, rename
/// detection, binary files flagged with no hunks.
///
/// # Errors
/// When git can't build or read the diff's patches.
pub fn diff_trees(repo: &Repository, old: &Tree, new: &Tree) -> Result<types::Diff> {
    let mut opts = DiffOptions::new();
    opts.context_lines(3);
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
        // New path; old path when deleted.
        let file_path = if delta.status() == Delta::Deleted {
            path(delta.old_file())
        } else {
            path(delta.new_file())
        };
        let old_path = (status == "renamed").then(|| path(delta.old_file()));

        let mut file = types::DiffFile {
            path: file_path,
            old_path,
            status: status.to_string(),
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
                    file.additions = i64::try_from(additions)?;
                    file.deletions = i64::try_from(deletions)?;
                    file.hunks = patch_hunks(&mut patch)?;
                }
            }
            // git2 yields no patch for binary entries in a tree diff.
            None => file.binary = true,
        }
        files.push(file);
    }
    Ok(types::Diff { files })
}

fn delta_status(delta: Delta) -> Option<&'static str> {
    match delta {
        Delta::Added => Some("added"),
        Delta::Deleted => Some("deleted"),
        Delta::Modified | Delta::Typechange => Some("modified"),
        Delta::Renamed | Delta::Copied => Some("renamed"),
        _ => None,
    }
}

fn patch_hunks(patch: &mut Patch) -> Result<Vec<types::Hunk>> {
    let mut hunks = Vec::new();
    for h in 0..patch.num_hunks() {
        let (hunk, _) = patch.hunk(h)?;
        let mut lines = Vec::new();
        for l in 0..patch.num_lines_in_hunk(h)? {
            let line = patch.line_in_hunk(h, l)?;
            let kind = match line.origin() {
                ' ' => "context",
                '+' => "add",
                '-' => "del",
                _ => continue, // eofnl markers etc.
            };
            let text = String::from_utf8_lossy(line.content());
            lines.push(types::Line {
                kind: kind.to_string(),
                old: line.old_lineno().map(i64::from),
                new: line.new_lineno().map(i64::from),
                text: text.strip_suffix('\n').unwrap_or(&text).to_string(),
            });
        }
        hunks.push(types::Hunk {
            old_start: i64::from(hunk.old_start()),
            old_lines: i64::from(hunk.old_lines()),
            new_start: i64::from(hunk.new_start()),
            new_lines: i64::from(hunk.new_lines()),
            header: hunk_function_context(hunk.header()),
            lines,
        });
    }
    Ok(hunks)
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

/// Port a line anchor from `old_tree` to `new_tree` (docs/api.md "Comment
/// rendering across revisions"): `Some(shifted)` when the line lies in an
/// unchanged region, `None` when the anchored line itself was changed or
/// deleted (or porting is impossible) — the `outdated` case.
///
/// # Errors
/// When git can't diff `file` between the trees.
pub fn port_line(
    repo: &Repository,
    old_tree: &Tree,
    new_tree: &Tree,
    file: &str,
    line: i64,
) -> Result<Option<i64>> {
    if line < 1 || old_tree.get_path(Path::new(file)).is_err() {
        return Ok(None); // anchor never existed on this side
    }
    let mut opts = DiffOptions::new();
    opts.context_lines(0)
        .disable_pathspec_match(true)
        .pathspec(file);
    let diff = repo.diff_tree_to_tree(Some(old_tree), Some(new_tree), Some(&mut opts))?;

    // No rename detection: a renamed file shows up as delete+add and the
    // anchor goes outdated — conservative, and `file` keeps its meaning.
    let mut offset = 0i64;
    for idx in 0..diff.deltas().len() {
        let delta = diff
            .get_delta(idx)
            .ok_or_else(|| anyhow!("diff delta {idx} vanished"))?;
        if delta.old_file().path() != Some(Path::new(file)) {
            continue;
        }
        if delta.status() == Delta::Deleted {
            return Ok(None);
        }
        let Some(patch) = Patch::from_diff(&diff, idx)? else {
            return Ok(None); // binary — no line mapping
        };
        if patch.delta().flags().is_binary() {
            return Ok(None);
        }
        for h in 0..patch.num_hunks() {
            let (hunk, _) = patch.hunk(h)?;
            let old_start = i64::from(hunk.old_start());
            let old_lines = i64::from(hunk.old_lines());
            let new_lines = i64::from(hunk.new_lines());
            if old_lines == 0 {
                // Pure insertion *after* old line `old_start`.
                if line <= old_start {
                    return Ok(Some(line + offset));
                }
            } else {
                if line < old_start {
                    return Ok(Some(line + offset));
                }
                if line < old_start + old_lines {
                    return Ok(None); // the anchored line itself changed
                }
            }
            offset += new_lines - old_lines;
        }
    }
    Ok(Some(line + offset))
}

/// Snapshot of line `line` (1-based) of `file` in `tree`, for
/// `comments.line_text`. `None` when the path/line/encoding make that
/// impossible.
pub fn line_text(repo: &Repository, tree: &Tree, file: &str, line: i64) -> Option<String> {
    if line < 1 {
        return None;
    }
    let entry = tree.get_path(Path::new(file)).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    if blob.is_binary() {
        return None;
    }
    let content = String::from_utf8_lossy(blob.content()).into_owned();
    let idx = usize::try_from(line - 1).ok()?;
    content.lines().nth(idx).map(str::to_string)
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
            let dir = tempfile::tempdir().unwrap();
            let mut opts = RepositoryInitOptions::new();
            opts.initial_head("refs/heads/main");
            let repo = Repository::init_opts(dir.path().join("r"), &opts).unwrap();
            Repo { _dir: dir, repo }
        }

        /// Build a tree from (path, content) pairs (bytes allow binary).
        fn tree(&self, files: &[(&str, &[u8])]) -> git2::Oid {
            let mut builder = self.repo.treebuilder(None).unwrap();
            for (path, content) in files {
                let blob = self.repo.blob(content).unwrap();
                builder.insert(path, blob, 0o100_644).unwrap();
            }
            builder.write().unwrap()
        }

        fn find(&self, oid: git2::Oid) -> Tree<'_> {
            self.repo.find_tree(oid).unwrap()
        }
    }

    fn lines(n: std::ops::RangeInclusive<i64>) -> String {
        use std::fmt::Write;
        n.fold(String::new(), |mut s, i| {
            writeln!(s, "line {i}").unwrap();
            s
        })
    }

    #[test]
    fn modified_file_hunks_and_line_numbers() {
        let r = Repo::new();
        let old = lines(1..=20);
        let new = old.replace("line 3\n", "line three\n").replace(
            "line 17\n",
            "line 17\nline 17.5\n", // insertion lower down
        );
        let t_old = r.tree(&[("a.txt", old.as_bytes())]);
        let t_new = r.tree(&[("a.txt", new.as_bytes())]);
        let diff = diff_trees(&r.repo, &r.find(t_old), &r.find(t_new)).unwrap();

        assert_eq!(diff.files.len(), 1);
        let f = &diff.files[0];
        assert_eq!(f.path, "a.txt");
        assert_eq!(f.old_path, None);
        assert_eq!(f.status, "modified");
        assert!(!f.binary);
        assert_eq!((f.additions, f.deletions), (2, 1));
        assert_eq!(f.hunks.len(), 2);

        let h0 = &f.hunks[0];
        assert_eq!(
            (h0.old_start, h0.old_lines, h0.new_start, h0.new_lines),
            (1, 6, 1, 6)
        );
        let del = h0.lines.iter().find(|l| l.kind == "del").unwrap();
        assert_eq!(
            (del.old, del.new, del.text.as_str()),
            (Some(3), None, "line 3")
        );
        let add = h0.lines.iter().find(|l| l.kind == "add").unwrap();
        assert_eq!(
            (add.old, add.new, add.text.as_str()),
            (None, Some(3), "line three")
        );
        let ctx = &h0.lines[0];
        assert_eq!(
            (ctx.kind.as_str(), ctx.old, ctx.new),
            ("context", Some(1), Some(1))
        );

        let h1 = &f.hunks[1];
        assert_eq!(h1.old_start, 15); // 3 context lines above the insertion
        let add = h1.lines.iter().find(|l| l.kind == "add").unwrap();
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
        let diff = diff_trees(&r.repo, &r.find(t_old), &r.find(t_new)).unwrap();

        let by_path = |p: &str| diff.files.iter().find(|f| f.path == p).unwrap();
        assert_eq!(diff.files.len(), 4); // keep.txt untouched

        let added = by_path("fresh.txt");
        assert_eq!(added.status, "added");
        assert_eq!((added.additions, added.deletions), (1, 0));
        let l = &added.hunks[0].lines[0];
        assert_eq!((l.kind.as_str(), l.old, l.new), ("add", None, Some(1)));

        let deleted = by_path("doomed.txt");
        assert_eq!(deleted.status, "deleted");
        assert_eq!((deleted.additions, deleted.deletions), (0, 1));

        let renamed = by_path("new_name.txt");
        assert_eq!(renamed.status, "renamed");
        assert_eq!(renamed.old_path.as_deref(), Some("old_name.txt"));

        let bin = by_path("bin.dat");
        assert!(bin.binary);
        assert!(bin.hunks.is_empty());
        assert_eq!((bin.additions, bin.deletions), (0, 0));
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
    fn port_line_shifts_and_outdates() {
        let r = Repo::new();
        let old = lines(1..=10);
        // Insert two lines after line 2, change line 7, delete line 9.
        let new = old
            .replace("line 2\n", "line 2\nins a\nins b\n")
            .replace("line 7\n", "line seven\n")
            .replace("line 9\n", "");
        let t_old = r.find(r.tree(&[("a.txt", old.as_bytes())]));
        let t_new = r.find(r.tree(&[("a.txt", new.as_bytes())]));
        let port = |line| port_line(&r.repo, &t_old, &t_new, "a.txt", line).unwrap();

        assert_eq!(port(1), Some(1)); // above all edits
        assert_eq!(port(2), Some(2)); // insertion is *after* line 2
        assert_eq!(port(3), Some(5)); // shifted by the two inserted lines
        assert_eq!(port(7), None); // the line itself changed
        assert_eq!(port(8), Some(10)); // between change and deletion
        assert_eq!(port(9), None); // deleted
        assert_eq!(port(10), Some(11)); // +2 -1
        assert_eq!(port(0), None); // nonsense anchor
    }

    #[test]
    fn port_line_file_level_cases() {
        let r = Repo::new();
        let t_a = r.find(r.tree(&[("a.txt", b"x\ny\n".as_slice())]));
        let t_b = r.find(r.tree(&[("b.txt", b"x\ny\n".as_slice())]));
        let t_a2 = r.find(r.tree(&[("a.txt", b"x\ny\n".as_slice())]));

        // Untouched file: identity.
        assert_eq!(
            port_line(&r.repo, &t_a, &t_a2, "a.txt", 2).unwrap(),
            Some(2)
        );
        // File deleted (rename without detection counts as deletion).
        assert_eq!(port_line(&r.repo, &t_a, &t_b, "a.txt", 1).unwrap(), None);
        // Anchor file absent on the old side.
        assert_eq!(port_line(&r.repo, &t_b, &t_a, "a.txt", 1).unwrap(), None);
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
}
