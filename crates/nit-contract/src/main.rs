//! Writes nit's public contract into the directory named by the first
//! argument: `openapi.json`, the document the routes' utoipa annotations
//! produce.

use std::path::PathBuf;

use anyhow::{Result, bail};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let [_, dir] = args.as_slice() else {
        bail!("usage: nit-contract <dir>");
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("openapi.json"),
        nit::api::openapi().to_pretty_json()?,
    )?;
    Ok(())
}
