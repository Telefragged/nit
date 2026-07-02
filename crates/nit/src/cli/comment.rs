//! `nit comment` — open a new thread on a change or reply to an existing one.

use anyhow::{Context, Result, anyhow};

use nit_types::comments::{CommentRange, NewComment, Thread};
use nit_types::enums::Side;

use super::client::{Client, ServerOpt, server_url};
use super::format::{ChangeTarget, print_comment};

#[derive(clap::Args)]
pub struct CommentArgs {
    #[command(flatten)]
    pub target: ChangeTarget,
    /// Reply to an existing thread on the change (by id) instead of opening
    /// a new one.
    #[arg(long)]
    pub thread: Option<u64>,
    /// New thread: file to anchor to (`--line`/`--range` require a
    /// `--file`).
    #[arg(long, conflicts_with = "thread")]
    pub file: Option<String>,
    /// New thread: line to anchor to (1-based).
    #[arg(long, conflicts_with = "thread")]
    pub line: Option<u64>,
    /// New thread: side — `new` (default) or `old`.
    #[arg(long, conflicts_with = "thread", value_enum)]
    pub side: Option<Side>,
    /// New thread: selected-text range `START-END`, each `line:char`;
    /// anchors the thread under END's line (mutually exclusive with
    /// `--line`).
    #[arg(long, conflicts_with_all = ["thread", "line"])]
    pub range: Option<String>,
    /// New thread: revision to anchor to (defaults to the change's latest).
    #[arg(long, conflicts_with = "thread")]
    pub revision: Option<u64>,
    /// Comment body, markdown (optional only for a `--thread` reply that
    /// just resolves/reopens).
    #[arg(short = 'm', long = "message")]
    pub message: Option<String>,
    /// Read the body from a file, or stdin for `-` — multi-line markdown
    /// without shell escaping.
    #[arg(short = 'F', long = "message-file", conflicts_with = "message")]
    pub message_file: Option<String>,
    /// Mark the thread resolved (a new thread is born resolved).
    #[arg(long)]
    pub resolve: bool,
    /// Reopen the thread
    #[arg(long, conflicts_with = "resolve")]
    pub unresolve: bool,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Comment on a change: open a new thread or reply to one.
///
/// # Errors
/// When the server can't be reached or the arguments name no change.
pub fn comment(args: CommentArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    let change_id = args.target.resolve(&client)?;
    let resolved = if args.resolve {
        Some(true)
    } else if args.unresolve {
        Some(false)
    } else {
        None
    };
    let range = args
        .range
        .map(|spec| parse_comment_range(&spec))
        .transpose()?;
    let body = match args.message_file {
        Some(path) => read_body(&path)?,
        None => args.message.unwrap_or_default(),
    };
    let replied = args.thread.is_some();
    let req = NewComment {
        thread_id: args.thread,
        revision: args.revision,
        file: args.file,
        line: args.line,
        side: args.side,
        range,
        body,
        resolved,
    };
    let thread: Thread = client.post(&format!("/api/changes/{change_id}/comments"), &req)?;
    print_comment(&thread, replied);
    Ok(())
}

/// Read a `-F` body: a file path, or stdin for `-`. Trailing newlines are
/// trimmed — a heredoc or editor always appends one, and it would read as
/// a trailing hard break in the rendered markdown.
fn read_body(path: &str) -> Result<String> {
    let mut text = if path == "-" {
        std::io::read_to_string(std::io::stdin()).context("reading body from stdin")?
    } else {
        std::fs::read_to_string(path).with_context(|| format!("reading body from {path}"))?
    };
    text.truncate(text.trim_end_matches(['\n', '\r']).len());
    Ok(text)
}

/// Parse a `--range` spec `START-END`, each endpoint `line:char`.
fn parse_comment_range(spec: &str) -> Result<CommentRange> {
    let (start, end) = spec
        .split_once('-')
        .ok_or_else(|| anyhow!("range must be START-END (e.g. 12:4-14:7), got {spec:?}"))?;
    let point = |s: &str| -> Result<(u64, u64)> {
        let (line, ch) = s
            .split_once(':')
            .ok_or_else(|| anyhow!("range endpoint must be line:char, got {s:?}"))?;
        Ok((
            line.trim()
                .parse()
                .with_context(|| format!("bad line in {s:?}"))?,
            ch.trim()
                .parse()
                .with_context(|| format!("bad char in {s:?}"))?,
        ))
    };
    let (start_line, start_char) = point(start)?;
    let (end_line, end_char) = point(end)?;
    Ok(CommentRange {
        start_line,
        start_char,
        end_line,
        end_char,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_comment_range_forms_and_rejections() {
        assert_eq!(
            parse_comment_range("12:4-14:7").expect("ok"),
            CommentRange {
                start_line: 12,
                start_char: 4,
                end_line: 14,
                end_char: 7,
            }
        );
        assert!(parse_comment_range("12:4").is_err());
        assert!(parse_comment_range("12-14").is_err());
        assert!(parse_comment_range("a:4-14:7").is_err());
    }
}
