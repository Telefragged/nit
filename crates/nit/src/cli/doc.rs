use std::fmt::Write as _;

use anyhow::{Context, Result};
use nit_document::wire::{
    CreateDocument, DocumentList, DocumentNumber, DocumentQuery, DocumentSummary,
};
use nit_document::{Document, Edit, Version, Writer};

use super::client::{Client, ServerOpt, server_url};
use super::comment::read_body;
use super::git::discover_repo;
use super::tags::{observed_tags, selection_tag};

#[derive(clap::Args)]
pub struct DocArgs {
    #[command(subcommand)]
    cmd: DocCmd,
    #[command(flatten)]
    server: ServerOpt,
}

#[derive(clap::Subcommand)]
enum DocCmd {
    /// Create a document with the tags of this checkout. Prints the number and the URL
    New {
        /// The title of the document
        #[arg(long)]
        title: String,
        /// The Markdown text of the document (`-` for stdin)
        #[arg(short = 'F', long = "file")]
        file: Option<String>,
    },
    /// Print one line per document of this session, worktree or branch
    List,
    /// Print the Markdown text of a document on stdout, exactly as stored, and its version on stderr
    Show {
        document: DocumentNumber,
        /// Print the lines that differ from this version as a diff. Each hunk header contains
        /// the 0-based first line in the old and the new text, and the writers of the hunk
        #[arg(long)]
        since: Option<Version>,
    },
    /// Replace one exact text of a document
    #[command(group = clap::ArgGroup::new("replacement").required(true))]
    Edit {
        document: DocumentNumber,
        /// Text that the document contains exactly once
        #[arg(long)]
        old: String,
        /// The text that replaces --old
        #[arg(long, group = "replacement")]
        new: Option<String>,
        /// A file with the text that replaces --old (`-` for stdin). The line breaks at the end
        /// of the file are not part of the text
        #[arg(short = 'F', long = "file", group = "replacement")]
        file: Option<String>,
    },
    /// Add text at the end of a document as a block of its own, after one empty line
    Append {
        document: DocumentNumber,
        /// The Markdown text to add (`-` for stdin)
        #[arg(short = 'F', long = "file")]
        file: String,
    },
}

/// Runs a `nit doc` subcommand.
///
/// # Errors
///
/// When the server refuses a request, a file is unreadable, or the
/// document does not accept the edit.
pub fn doc(args: DocArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    match args.cmd {
        DocCmd::New { title, file } => new(&client, title, file.as_deref()),
        DocCmd::List => list(&client),
        DocCmd::Show { document, since } => show(&client, document, since.as_ref()),
        DocCmd::Edit {
            document,
            old,
            new,
            file,
        } => {
            let new = match (new, file) {
                (Some(new), _) => new,
                (None, Some(file)) => read_body(&file)?,
                (None, None) => unreachable!("clap requires --new or -F"),
            };
            edit(&client, document, |replica| {
                Ok(replica.editor(Writer::Author).replace(&old, &new)?)
            })
        }
        DocCmd::Append { document, file } => {
            let text = read_text(&file)?;
            edit(&client, document, |replica| {
                replica.editor(Writer::Author).append(&text);
                Ok(())
            })
        }
    }
}

fn new(client: &Client, title: String, file: Option<&str>) -> Result<()> {
    let (_, repo) = discover_repo()?;
    let text = file.map(read_text).transpose()?;
    let created: DocumentSummary = client.post(
        "/api/documents",
        &CreateDocument {
            title,
            tags: observed_tags(&repo, true),
        },
    )?;
    if let Some(text) = text {
        let mut replica = Document::new();
        replica.editor(Writer::Author).append(&text);
        post(client, created.number, &replica, &created.version)?;
    }
    println!(
        "document {}  {}",
        created.number,
        client.page(&format!("/documents/{}", created.number))
    );
    Ok(())
}

fn list(client: &Client) -> Result<()> {
    let (_, repo) = discover_repo()?;
    let query = serde_html_form::to_string(DocumentQuery {
        tag: selection_tag(&repo).into_iter().collect(),
    })?;
    let list: DocumentList = client.get(&format!("/api/documents?{query}"))?;
    for document in list.documents {
        println!(
            "{}  {}  {}",
            document.number, document.version, document.title
        );
    }
    Ok(())
}

fn show(client: &Client, document: DocumentNumber, since: Option<&Version>) -> Result<()> {
    let replica = load(client, document)?;
    let version = replica.version();
    if let Some(since) = since {
        print!("{}", render_edits(&replica.edits_since(since)?));
        eprintln!("document {document}  version {version}  since {since}");
    } else {
        print!("{}", replica.text());
        eprintln!("document {document}  version {version}");
    }
    Ok(())
}

pub(crate) fn load(client: &Client, number: DocumentNumber) -> Result<Document> {
    let snapshot = client.get_bytes(&format!("/api/documents/{number}/snapshot"))?;
    Ok(Document::load(&snapshot)?)
}

fn edit(
    client: &Client,
    number: DocumentNumber,
    edit: impl FnOnce(&mut Document) -> Result<()>,
) -> Result<()> {
    let mut replica = load(client, number)?;
    let before = replica.version();
    edit(&mut replica)?;
    post(client, number, &replica, &before)
}

fn post(
    client: &Client,
    number: DocumentNumber,
    replica: &Document,
    since: &Version,
) -> Result<()> {
    let _: DocumentSummary = client.post_bytes(
        &format!("/api/documents/{number}/updates"),
        replica.updates_since(since).as_bytes(),
    )?;
    Ok(())
}

pub(super) fn read_text(file: &str) -> Result<String> {
    if file == "-" {
        std::io::read_to_string(std::io::stdin()).context("reading the text from stdin")
    } else {
        std::fs::read_to_string(file).with_context(|| format!("reading the text from {file}"))
    }
}

pub(crate) fn render_edits(edits: &[Edit]) -> String {
    let mut out = String::new();
    for edit in edits {
        let by: Vec<&str> = edit.by.iter().map(|writer| writer.as_str()).collect();
        writeln!(
            out,
            "@@ -{},{} +{},{} @@ by {}",
            edit.old_line,
            edit.removed.len(),
            edit.new_line,
            edit.added.len(),
            by.join(", ")
        )
        .expect("a string accepts text");
        let lines = edit
            .removed
            .iter()
            .map(|line| ('-', line))
            .chain(edit.added.iter().map(|line| ('+', line)));
        for (sign, line) in lines {
            let line = line.strip_suffix('\n').unwrap_or(line);
            writeln!(out, "{sign}{line}").expect("a string accepts text");
        }
    }
    out
}
