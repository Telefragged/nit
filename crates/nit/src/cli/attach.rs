//! `nit attach` — puts a file on a revision of a change.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::Repository;

use nit_types::attachments::UploadedAttachment;
use nit_types::changes::ChangeDetail;
use nit_types::domain::{AttachPayload, AttachmentName, RevisionNumber};

use super::client::{Client, ServerOpt, server_url};
use super::format::ChangeTarget;
use super::git::canonical_workdir;

#[derive(clap::Args)]
pub struct AttachArgs {
    /// The file to attach.
    pub file: PathBuf,
    #[command(flatten)]
    pub target: ChangeTarget,
    /// The revision to attach to. Default: the change's latest revision.
    #[arg(long)]
    pub revision: Option<RevisionNumber>,
    /// The name of the attachment, a relative path. Default: the file's path from the repository root, or its file name outside a repository.
    #[arg(long)]
    pub name: Option<String>,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Uploads a file and attaches it to a revision of a change.
///
/// # Errors
///
/// When the file cannot be read, the name is not a relative path, or the
/// server refuses the upload or the attach.
pub fn attach(args: AttachArgs) -> Result<()> {
    let name = AttachmentName::new(match args.name {
        Some(name) => name,
        None => default_name(&args.file)?,
    })?;
    let client = Client::new(server_url(args.server.server));
    let change_number = args.target.resolve(&client)?;
    let revision = if let Some(revision) = args.revision {
        revision
    } else {
        let detail: ChangeDetail = client.get(&format!("/api/changes/{change_number}"))?;
        detail
            .revisions
            .last()
            .context("the change has no revision")?
            .number
    };
    let file = std::fs::File::open(&args.file)
        .with_context(|| format!("opening {}", args.file.display()))?;
    let uploaded: UploadedAttachment = client.put_file("/api/attachments", file)?;
    let request = AttachPayload {
        revision,
        name,
        hash: uploaded.hash,
    };
    let _: ChangeDetail = client.post(
        &format!("/api/changes/{change_number}/attachments"),
        &request,
    )?;
    println!(
        "change {change_number} r{revision} attached {} {}",
        request.name,
        &request.hash.as_str()[..12]
    );
    Ok(())
}

/// The file's path from the root of the repository that holds it, or its
/// file name when no repository holds it.
fn default_name(file: &Path) -> Result<String> {
    let path = file
        .canonicalize()
        .with_context(|| format!("reading {}", file.display()))?;
    let from_root = Repository::discover(&path)
        .ok()
        .and_then(|repo| canonical_workdir(&repo))
        .and_then(|root| Some(path.strip_prefix(root).ok()?.to_path_buf()));
    let name = match from_root {
        Some(relative) => relative,
        None => PathBuf::from(path.file_name().context("the file has no name")?),
    };
    name.to_str()
        .map(str::to_owned)
        .context("the file's path is not UTF-8")
}
