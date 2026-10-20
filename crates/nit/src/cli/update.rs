//! `nit update` — replace this nit with the latest release.

use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

/// The script that the release publishes, as this build carries it.
static INSTALL: &str = include_str!("../../install.sh");

/// Installs the latest release over the binary that runs this command.
///
/// # Errors
///
/// When nix installed this binary, or when the install script fails.
pub fn update() -> Result<()> {
    let exe = std::env::current_exe().context("locating this nit")?;
    if exe.starts_with("/nix/store") {
        bail!(
            "nix installed {}, so update nit through nix: update the flake input and switch, \
             or run `nix profile upgrade nit`",
            exe.display()
        );
    }
    let dir = exe.parent().context("locating this nit's directory")?;
    // A user who installed by hand chose their own PATH.
    let status = Command::new("sh")
        .args(["-c", INSTALL])
        .env("NIT_INSTALL_DIR", dir)
        .env("NIT_NO_MODIFY_PATH", "1")
        .status()
        .context("running sh")?;
    ensure!(status.success(), "the install script failed: {status}");
    Ok(())
}
