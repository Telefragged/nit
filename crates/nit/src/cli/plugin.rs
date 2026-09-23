//! `nit install-plugin` — install the Claude Code plugin that this build
//! carries.
//!
//! The plugin lives in a store at `$XDG_DATA_HOME/nit/claude-plugin/`. The
//! store holds one directory per nit version and a `current` symlink to one
//! of them. Claude Code reads `current` as the `nit` marketplace. An install
//! takes three steps, and after each step `current` points at a complete
//! plugin:
//!
//! 1. Remove every entry except `current` and the version that it points at.
//! 2. Write the plugin into a temporary directory and sync it. Then rename
//!    the directory to its version.
//! 3. Make a temporary symlink to that version and rename it over `current`.
//!
//! The cleanup runs first, so the previous version stays until the next
//! install. A Claude Code process that copies it at the same time can finish.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};

static PLUGIN: include_dir::Dir = include_dir::include_dir!("$NIT_PLUGIN_DIR");

/// Installs this build's plugin into the store and registers it with Claude
/// Code.
///
/// # Errors
///
/// When Claude Code has no config directory, before anything is written.
/// When a write to the store fails, or when `claude` is missing or fails.
pub fn install_plugin() -> Result<()> {
    let config = claude_config_dir()?;
    ensure!(
        config.is_dir(),
        "Claude Code was not found at {}",
        config.display()
    );
    let current = install_to(&crate::db::nit_data_dir()?.join("claude-plugin"))?;
    run(Command::new("claude")
        .args(["plugin", "marketplace", "add"])
        .arg(&current))?;
    run(Command::new("claude").args(["plugin", "install", "nit@nit"]))
}

/// Claude Code's config directory: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
fn claude_config_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Ok(dir.into());
    }
    let home = std::env::var_os("HOME").context("$HOME is not set")?;
    Ok(PathBuf::from(home).join(".claude"))
}

/// Writes this build's plugin into `store` and points `store/current` at it.
///
/// Returns the path of `current`.
fn install_to(store: &Path) -> Result<PathBuf> {
    fs::create_dir_all(store).with_context(|| format!("creating {}", store.display()))?;
    remove_stale(store)?;
    let version = store.join(crate::VERSION);
    let tmp = store.join(format!(".tmp-{}", std::process::id()));
    // A version directory that survives the cleanup is the target of
    // `current`, so it is complete.
    if !version.exists() {
        write_plugin(&tmp)?;
        fs::rename(&tmp, &version).context("renaming the plugin to its version")?;
        // Without this sync, a crash can keep the rename of `current` but
        // lose this one.
        sync(store)?;
    }
    let link = tmp.with_extension("link");
    let current = store.join("current");
    std::os::unix::fs::symlink(crate::VERSION, &link).context("linking the plugin")?;
    fs::rename(&link, &current).context("replacing the current plugin")?;
    sync(store)?;
    Ok(current)
}

/// Removes every entry of `store` except `current` and its target.
fn remove_stale(store: &Path) -> Result<()> {
    let target = fs::read_link(store.join("current")).ok();
    for entry in fs::read_dir(store)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "current" || target.as_deref() == Some(name.as_ref()) {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        }
        .with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(())
}

/// Writes the plugin and its marketplace manifest into `dir`, synced to disk.
fn write_plugin(dir: &Path) -> Result<()> {
    fs::create_dir(dir).with_context(|| format!("creating {}", dir.display()))?;
    PLUGIN.extract(dir).context("writing the plugin")?;
    let manifest = serde_json::json!({
        "name": "nit",
        "owner": { "name": "Telefragged" },
        "plugins": [{ "name": "nit", "source": "./", "version": crate::VERSION }],
    });
    fs::write(
        dir.join(".claude-plugin/marketplace.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )
    .context("writing the marketplace manifest")?;
    sync_tree(dir)
}

/// Syncs every file and directory under `path` to disk, children first.
fn sync_tree(path: &Path) -> Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            sync_tree(&entry?.path())?;
        }
    }
    sync(path)
}

fn sync(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .with_context(|| format!("syncing {}", path.display()))
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {cmd:?}"))?;
    ensure!(status.success(), "{cmd:?} exited with {status}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::os::unix::fs::symlink;

    fn entries(store: &Path) -> BTreeSet<String> {
        fs::read_dir(store)
            .expect("read store")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn install_keeps_the_previous_version_until_the_next_install() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = dir.path();
        fs::create_dir(store.join("0.0.1")).expect("previous");
        symlink("0.0.1", store.join("current")).expect("current");
        fs::create_dir(store.join("0.0.0")).expect("stale version");
        fs::write(store.join(".tmp-1"), "").expect("stale temp");

        let current = install_to(store).expect("install");
        let installed = BTreeSet::from(["current", "0.0.1", crate::VERSION].map(String::from));
        assert_eq!(entries(store), installed);
        assert_eq!(
            fs::read_link(&current).expect("link"),
            Path::new(crate::VERSION)
        );
        assert!(current.join(".claude-plugin/plugin.json").is_file());
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(current.join(".claude-plugin/marketplace.json")).expect("manifest"),
        )
        .expect("json");
        assert_eq!(manifest["plugins"][0]["version"], crate::VERSION);

        install_to(store).expect("reinstall");
        let reinstalled = BTreeSet::from(["current", crate::VERSION].map(String::from));
        assert_eq!(entries(store), reinstalled);
    }
}
