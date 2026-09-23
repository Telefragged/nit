//! Stamps the version with `+<sha>[.dirty]` from the build-time git state, so
//! `nit --version` and `/api/health` name the exact build. The flake passes
//! `NIT_GIT_SUFFIX` for sandboxed nix builds (no `.git` reachable); a plain
//! `cargo` build derives it from the working tree here. With no `rerun-if-*`,
//! Cargo re-runs this only when it recompiles the nit crate, so a dev build's
//! dirty flag is best-effort — a git change outside the crate won't re-stamp
//! until the crate rebuilds. nix recomputes every build, so release stamps are
//! exact.
//!
//! It also compiles the built web UI into the binary when `NIT_WEB_DIST` names
//! its directory. The flake sets it; a plain `cargo` build leaves it unset and
//! serves the API only.
//!
//! The Claude Code plugin is always compiled in, from `NIT_PLUGIN_DIR`. The
//! flake sets it, and a plain `cargo` build reads the repo's `plugins/nit`.

use std::process::Command;

fn main() {
    let suffix = std::env::var("NIT_GIT_SUFFIX")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(git_suffix)
        .unwrap_or_default();
    println!("cargo:rustc-env=NIT_GIT_SUFFIX={suffix}");
    println!("cargo::rustc-check-cfg=cfg(nit_web_ui)");
    if std::env::var_os("NIT_WEB_DIST").is_some() {
        println!("cargo::rustc-cfg=nit_web_ui");
    }
    if std::env::var_os("NIT_PLUGIN_DIR").is_none() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/nit");
        println!("cargo:rustc-env=NIT_PLUGIN_DIR={dir}");
    }
}

/// `+<short-sha>[.dirty]`, or `None` outside a git tree (a tarball build).
fn git_suffix() -> Option<String> {
    let sha = git(&["rev-parse", "--short=12", "HEAD"])?;
    let dirty = if git(&["status", "--porcelain"])?.is_empty() {
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
