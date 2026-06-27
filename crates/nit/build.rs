//! Stamps the version with `+<sha>[.dirty]` from the build-time git state, so
//! `nit --version` and `/api/health` name the exact build. The flake passes
//! `NIT_GIT_SUFFIX` for sandboxed nix builds (no `.git` reachable); a plain
//! `cargo` build derives it from the working tree here. Emitting no
//! `rerun-if-*` keeps Cargo's default whole-package watch, so an edit re-stamps
//! the dirty flag.

use std::process::Command;

fn main() {
    let suffix = std::env::var("NIT_GIT_SUFFIX")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(git_suffix)
        .unwrap_or_default();
    println!("cargo:rustc-env=NIT_GIT_SUFFIX={suffix}");
}

/// `+<short-sha>[.dirty]`, or `None` outside a git tree (a tarball build).
fn git_suffix() -> Option<String> {
    let sha = git(&["rev-parse", "--short=12", "HEAD"])?;
    // Tracked changes only: untracked files (build outputs, sibling worktrees)
    // aren't in the build and would diverge from the flake's `dirtyRev`.
    let dirty = if git(&["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        ""
    } else {
        ".dirty"
    };
    Some(format!("+{sha}{dirty}"))
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
