//! Differential test: nit's fixup attachment must mirror
//! `git rebase -i --autosquash`. For each layout we build a real repo with
//! the git CLI, capture the rearranged todo via `GIT_SEQUENCE_EDITOR` (the
//! editor copies it and exits non-zero so the rebase aborts untouched),
//! and compare every commit's group root against `attach_fixups`.

mod common;

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use nit::gitscan::fixup::{self, CommitMeta};

/// One commit in a layout: a literal subject, or a fixup targeting an
/// earlier commit by abbreviated sha.
enum Spec {
    S(&'static str),
    FixupSha(usize),
}
use Spec::{FixupSha, S};

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git must be runnable (devShell)")
}

fn git_ok(dir: &Path, args: &[&str]) {
    let out = git(dir, args);
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Build the repo, run autosquash, return (commit shas in branch order,
/// todo-derived root per sha, our root per sha).
#[expect(
    clippy::type_complexity,
    reason = "the differential triple (shas, git's roots, our roots)"
)]
fn run_layout(
    layout: &[Spec],
) -> (
    Vec<String>,
    HashMap<String, Option<String>>,
    HashMap<String, Option<String>>,
) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("repo");
    std::fs::create_dir(&dir).unwrap();
    git_ok(&dir, &["init", "-b", "main"]);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    git_ok(&dir, &["add", "-A"]);
    git_ok(&dir, &["commit", "-m", "base"]);
    git_ok(&dir, &["checkout", "-q", "-b", "feat"]);

    let mut shas: Vec<String> = Vec::new();
    for (i, spec) in layout.iter().enumerate() {
        let subject = match spec {
            S(s) => (*s).to_string(),
            FixupSha(target) => format!("fixup! {}", &shas[*target][..7]),
        };
        std::fs::write(dir.join(format!("f{i}.txt")), format!("content {i}\n")).unwrap();
        git_ok(&dir, &["add", "-A"]);
        git_ok(&dir, &["commit", "-m", &subject]);
        let out = git(&dir, &["rev-parse", "HEAD"]);
        shas.push(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }

    // Capture the autosquash-rearranged todo, then abort the rebase.
    let todo_out = tmp.path().join("todo");
    let editor = tmp.path().join("capture-todo.sh");
    std::fs::write(
        &editor,
        format!("#!/bin/sh\ncp \"$1\" \"{}\"\nexit 1\n", todo_out.display()),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&editor).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&editor, perms).unwrap();

    let out = Command::new("git")
        .current_dir(&dir)
        .args(["rebase", "-i", "--autosquash", "main"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_SEQUENCE_EDITOR", editor.to_str().unwrap())
        .output()
        .unwrap();
    assert!(!out.status.success(), "rebase must abort at the editor");

    // Parse the todo: a fixup/squash line's root is the group's pick.
    let todo = std::fs::read_to_string(&todo_out).expect("todo captured");
    let mut todo_roots: HashMap<String, Option<String>> = HashMap::new();
    let mut last_pick: Option<String> = None;
    for line in todo.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let cmd = words.next().unwrap();
        let mut sha = words.next().unwrap();
        if sha == "-C" || sha == "-c" {
            sha = words.next().unwrap();
        }
        let out = git(&dir, &["rev-parse", sha]);
        let full = String::from_utf8_lossy(&out.stdout).trim().to_string();
        match cmd {
            "pick" => {
                todo_roots.insert(full.clone(), None);
                last_pick = Some(full);
            }
            "fixup" | "squash" => {
                todo_roots.insert(
                    full,
                    Some(last_pick.clone().expect("fixup before any pick")),
                );
            }
            other => panic!("unexpected todo command {other:?} in: {line}"),
        }
    }
    assert_eq!(
        todo_roots.len(),
        layout.len(),
        "todo covers every commit:\n{todo}"
    );

    // Our attachment over the same walk.
    let repo = git2::Repository::open(&dir).unwrap();
    let metas: Vec<CommitMeta> = shas
        .iter()
        .map(|sha| {
            let commit = repo.find_commit(git2::Oid::from_str(sha).unwrap()).unwrap();
            CommitMeta {
                sha: sha.clone(),
                subject: fixup::subject_of(&String::from_utf8_lossy(commit.message_bytes())),
            }
        })
        .collect();
    let resolver = |needle: &str| {
        repo.revparse_single(needle)
            .ok()
            .and_then(|o| o.peel_to_commit().ok())
            .map(|c| c.id().to_string())
    };
    let roots = fixup::attach_fixups(&metas, resolver);
    let our_roots: HashMap<String, Option<String>> = shas
        .iter()
        .enumerate()
        .map(|(i, sha)| (sha.clone(), roots[i].map(|r| shas[r].clone())))
        .collect();

    (shas, todo_roots, our_roots)
}

fn assert_layout_matches(layout: &[Spec]) {
    let (shas, todo_roots, our_roots) = run_layout(layout);
    for (i, sha) in shas.iter().enumerate() {
        assert_eq!(
            our_roots[sha], todo_roots[sha],
            "commit #{i} ({sha}): nit attachment diverges from git autosquash"
        );
    }
}

#[test]
fn basic_fixup_chains() {
    assert_layout_matches(&[
        S("add api"),
        S("fixup! add api"),
        S("fixup! fixup! add api"),
        S("other thing"),
        S("fixup! other thing"),
    ]);
}

#[test]
fn duplicate_subjects_oldest_wins() {
    assert_layout_matches(&[
        S("dup"),
        S("dup"),
        S("fixup! dup"),
        S("third"),
        S("squash! dup"),
    ]);
}

#[test]
fn exact_match_beats_prefix_match() {
    assert_layout_matches(&[
        S("add api endpoint"),
        S("add api"),
        S("fixup! add api"),   // exact → "add api"
        S("fixup! add api e"), // prefix → "add api endpoint"
    ]);
}

#[test]
fn needle_longer_than_subject_does_not_prefix_match() {
    assert_layout_matches(&[S("a b"), S("a"), S("fixup! a b c"), S("fixup! a")]);
}

#[test]
fn sha_targeting_including_fixup_of_fixup() {
    assert_layout_matches(&[
        S("alpha"),
        S("beta"),
        FixupSha(0), // fixup! <sha of alpha>
        FixupSha(2), // fixup targeting the fixup commit → root alpha
    ]);
}

#[test]
fn untargeted_fixups_stay_regular() {
    assert_layout_matches(&[
        S("alpha"),
        S("fixup! nonexistent thing"),
        S("fixup! fixup! nonexistent thing"),
    ]);
}

#[test]
fn interleaved_squash_and_fixup_groups() {
    assert_layout_matches(&[
        S("one"),
        S("two"),
        S("squash! one"),
        S("fixup! two"),
        S("fixup! squash! one"), // strips to "one" → root one
        S("fixup! one"),
    ]);
}

#[test]
fn fixup_first_in_branch_cannot_target_later() {
    assert_layout_matches(&[
        S("fixup! later thing"),
        S("later thing"),
        S("fixup! later thing"),
    ]);
}
