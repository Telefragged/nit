//! Where the executables that nit runs are.

use std::path::PathBuf;

/// The first executable named `name` on `PATH`, or `None` when there is
/// none.
pub(crate) fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}
