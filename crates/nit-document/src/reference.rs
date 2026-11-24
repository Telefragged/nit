use nit_types::domain::{AttachmentHash, ChangeNumber, RevisionNumber, Side};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use pulldown_cmark::{Event, Parser, Tag};
use serde::{Deserialize, Serialize};

/// What a link on a line of its own shows on the document page.
///
/// Its text form is an address on the nit server. References to
/// changes have the address of the review page:
///
/// ```rust
/// use nit_document::Reference;
///
/// let card: Reference = "/changes/412".parse().unwrap();
/// assert!(matches!(card, Reference::Change { .. }));
///
/// let lines = "/changes/412?revision=3&file=src%2Ffold.rs&line=40-72";
/// let file: Reference = lines.parse().unwrap();
/// assert_eq!(file.to_string(), lines);
///
/// assert!("/chains/7".parse::<Reference>().is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Reference {
    /// A card for a change: its subject, its status and a link.
    Change { change: ChangeNumber },
    /// One file or attachment of a change at one revision, or a line
    /// range in that file. With `against`, the lines are those of the
    /// interdiff from that revision.
    File {
        change: ChangeNumber,
        revision: RevisionNumber,
        against: Option<RevisionNumber>,
        file: String,
        lines: Option<LineRange>,
    },
    /// A file uploaded to the document itself.
    Upload { hash: AttachmentHash, name: String },
}

/// An address that is not a [`Reference`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not the address of a reference")]
pub struct ReferenceError(String);

/// A range of lines on one side of a diff. Lines count from 1, and the
/// range includes `end`.
///
/// ```rust
/// use nit_document::LineRange;
/// use nit_types::domain::Side;
///
/// assert!(LineRange::new(Side::New, 40, 72).is_ok());
/// assert!(LineRange::new(Side::New, 0, 3).is_err());
/// assert!(LineRange::new(Side::Old, 9, 8).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Lines")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LineRange {
    side: Side,
    start: u64,
    end: u64,
}

#[derive(Deserialize)]
struct Lines {
    side: Side,
    start: u64,
    end: u64,
}

impl LineRange {
    /// The lines `start` to `end` of `side`.
    ///
    /// # Errors
    ///
    /// [`LineRangeError`], which identifies the rule the input broke.
    pub fn new(side: Side, start: u64, end: u64) -> Result<LineRange, LineRangeError> {
        if start < 1 {
            return Err(LineRangeError::LineBefore1);
        }
        if end < start {
            return Err(LineRangeError::EndsBeforeItsStart);
        }
        Ok(LineRange { side, start, end })
    }

    #[must_use]
    pub fn side(self) -> Side {
        self.side
    }

    #[must_use]
    pub fn start(self) -> u64 {
        self.start
    }

    #[must_use]
    pub fn end(self) -> u64 {
        self.end
    }
}

impl TryFrom<Lines> for LineRange {
    type Error = LineRangeError;

    fn try_from(lines: Lines) -> Result<LineRange, LineRangeError> {
        LineRange::new(lines.side, lines.start, lines.end)
    }
}

/// Why three values are not a [`LineRange`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LineRangeError {
    #[error("lines count from 1")]
    LineBefore1,
    #[error("a line range ends at or after its first line")]
    EndsBeforeItsStart,
}

const CHANGES: &str = "/changes/";
const UPLOADS: &str = "/api/attachments/";
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

impl std::fmt::Display for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reference::Change { change } => write!(f, "{CHANGES}{change}"),
            Reference::File {
                change,
                revision,
                against,
                file,
                lines,
            } => {
                let mut query = form_urlencoded::Serializer::new(String::new());
                query.append_pair("revision", &revision.to_string());
                if let Some(against) = against {
                    query.append_pair("against", &format!("r{against}"));
                }
                query.append_pair("file", file);
                if let Some(lines) = lines {
                    let (start, end) = (lines.start, lines.end);
                    let line = if start == end {
                        start.to_string()
                    } else {
                        format!("{start}-{end}")
                    };
                    query.append_pair("line", &line);
                    if lines.side == Side::Old {
                        query.append_pair("side", "old");
                    }
                }
                write!(f, "{CHANGES}{change}?{}", query.finish())
            }
            Reference::Upload { hash, name } => write!(
                f,
                "{UPLOADS}{}/{}",
                hash.as_str(),
                utf8_percent_encode(name, SEGMENT)
            ),
        }
    }
}

impl std::str::FromStr for Reference {
    type Err = ReferenceError;

    /// Parses the address of a reference, with or without the origin of
    /// the server. Parameters that no reference has are ignored.
    fn from_str(s: &str) -> Result<Reference, ReferenceError> {
        parse(s).ok_or_else(|| ReferenceError(s.to_owned()))
    }
}

fn parse(address: &str) -> Option<Reference> {
    let address = without_origin(address);
    let address = address
        .split_once('#')
        .map_or(address, |(before, _)| before);
    let (path, query) = address.split_once('?').unwrap_or((address, ""));
    if let Some(upload) = path.strip_prefix(UPLOADS) {
        let (hash, name) = upload.split_once('/')?;
        let name = percent_decode_str(name).decode_utf8().ok()?;
        if name.is_empty() || name.contains('/') {
            return None;
        }
        return Some(Reference::Upload {
            hash: AttachmentHash::new(hash).ok()?,
            name: name.into_owned(),
        });
    }
    let change = ChangeNumber::new(path.strip_prefix(CHANGES)?.parse().ok()?);
    let value = |key: &str| {
        form_urlencoded::parse(query.as_bytes())
            .find(|(k, _)| k == key)
            .map(|(_, value)| value.into_owned())
    };
    let Some(file) = value("file") else {
        return Some(Reference::Change { change });
    };
    let revision = RevisionNumber::new(value("revision")?.parse().ok()?);
    let against = match value("against") {
        Some(against) => Some(RevisionNumber::new(
            against.strip_prefix('r')?.parse().ok()?,
        )),
        None => None,
    };
    let side = if value("side").as_deref() == Some("old") {
        Side::Old
    } else {
        Side::New
    };
    let lines = match value("line") {
        Some(line) => {
            let (start, end) = line.split_once('-').unwrap_or((&line, &line));
            Some(LineRange::new(side, start.parse().ok()?, end.parse().ok()?).ok()?)
        }
        None => None,
    };
    Some(Reference::File {
        change,
        revision,
        against,
        file,
        lines,
    })
}

fn without_origin(address: &str) -> &str {
    let Some(rest) = address
        .strip_prefix("http://")
        .or_else(|| address.strip_prefix("https://"))
    else {
        return address;
    };
    rest.find('/').map_or("", |path| &rest[path..])
}

/// A reference in a text, with the place of its paragraph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PlacedReference {
    /// The first line of the paragraph in the text, counted from 0.
    pub line: usize,
    pub reference: Reference,
}

/// The references in the Markdown `text`, in order. A reference is a
/// top-level paragraph that is one link, one image, or one address
/// with the origin of a server, and whose address parses as a
/// [`Reference`]. Every text has a list.
#[must_use]
pub fn references(text: &str) -> Vec<PlacedReference> {
    #[derive(Default)]
    struct Paragraph {
        links: Vec<String>,
        bare: String,
        others: usize,
    }

    impl Paragraph {
        fn reference(&self) -> Option<Reference> {
            let bare = self.bare.trim();
            match (&self.links[..], self.others) {
                ([link], 0) if bare.is_empty() => parse(link),
                ([], 0) if bare.starts_with("http") && !bare.contains(char::is_whitespace) => {
                    parse(bare)
                }
                _ => None,
            }
        }
    }

    let mut references = Vec::new();
    let mut depth = 0usize;
    let mut paragraph: Option<Paragraph> = None;
    let (mut line, mut counted) = (0, 0);
    for (event, range) in Parser::new(text).into_offset_iter() {
        match (event, &mut paragraph) {
            (Event::Start(Tag::Paragraph), None) if depth == 0 => {
                line += text[counted..range.start].matches('\n').count();
                counted = range.start;
                paragraph = Some(Paragraph::default());
                depth = 1;
            }
            (Event::Start(tag), open) => {
                if let (Some(open), 1) = (open, depth) {
                    match tag {
                        Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                            open.links.push(dest_url.into_string());
                        }
                        _ => open.others += 1,
                    }
                }
                depth += 1;
            }
            (Event::End(_), open) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let reference = open.take().and_then(|open| open.reference());
                    references
                        .extend(reference.map(|reference| PlacedReference { line, reference }));
                }
            }
            (Event::Text(text), Some(open)) if depth == 1 => open.bare.push_str(&text),
            (_, Some(open)) if depth == 1 => open.others += 1,
            _ => {}
        }
    }
    references
}
