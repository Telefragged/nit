//! Where the executables that nit runs are, and how a running nit
//! re-executes a newer build of itself.
//!
//! An update replaces the binary on disk while the old process runs.
//! `nit watch` finds the new build by the path it started from, so that
//! path must not have its symlinks resolved. A nix profile entry and the curl
//! install's rename both put the new build at the old path, while
//! `/proc/self/exe` keeps naming the old file.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The first executable named `name` on `PATH`, or `None` when there is
/// none.
pub(crate) fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// The path this process started from, without its symlinks resolved.
///
/// That is `argv[0]` when it names a path, else the first executable of
/// that name on `PATH`. Returns `None` when neither gives a file.
pub(crate) fn invoked() -> Option<PathBuf> {
    let arg0 = PathBuf::from(std::env::args_os().next()?);
    if arg0.components().count() > 1 {
        return Some(arg0);
    }
    on_path(arg0.to_str()?)
}

/// The version that the binary at `path` reports, or `None` when it does
/// not run.
pub(crate) fn version_of(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("--client-version").output().ok()?;
    let version = String::from_utf8(output.stdout).ok()?;
    output
        .status
        .success()
        .then(|| version.trim_end().to_owned())
}

/// Replaces this process with the binary at `path`, run with the
/// arguments of this process.
///
/// The process keeps its id, and with it its parent. Returns only when
/// the exec fails.
pub(crate) fn exec(path: &Path) -> std::io::Error {
    Command::new(path).args(std::env::args_os().skip(1)).exec()
}
