use std::collections::{BTreeSet, HashMap};

use loro::cursor::Side;
use loro::{ExportMode, Frontiers, ID, LoroDoc, LoroText, LoroValue, PeerID, VersionVector};
use serde::{Deserialize, Serialize};
use similar::{DiffTag, TextDiff};

const TEXT: &str = "text";
const WRITERS: &str = "writers";

/// The side that made an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Writer {
    /// The authors of the changes, who edit through the CLI.
    Author,
    /// The reviewers, who edit in the browser.
    Reviewer,
}

impl Writer {
    const ALL: [Writer; 2] = [Writer::Author, Writer::Reviewer];

    /// The name of the writer, as the wire spells it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Writer::Author => "author",
            Writer::Reviewer => "reviewer",
        }
    }
}

/// One replica of a document.
///
/// Writers edit a replica through [`Document::editor`]. Replicas
/// exchange what they lack with [`Document::updates_since`] and
/// [`Document::merge`], in any order and any number of times, and then
/// have equal texts.
pub struct Document {
    doc: LoroDoc,
}

/// A replica with the writer that edits it.
pub struct Editor<'a> {
    document: &'a mut Document,
    writer: Writer,
}

impl Default for Document {
    fn default() -> Document {
        Document::new()
    }
}

/// A point in the history of a document: the edits that a replica
/// contains. Its text form is the last edit of each concurrent branch,
/// for example `41@7`, or `-` for a document with no edit.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "string"))]
pub struct Version(Frontiers);

/// A string that is not a [`Version`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a document version")]
pub struct VersionError(String);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut ids: Vec<ID> = self.0.iter().collect();
        if ids.is_empty() {
            return f.write_str("-");
        }
        ids.sort();
        let ids: Vec<String> = ids
            .iter()
            .map(|id| format!("{}@{}", id.counter, id.peer))
            .collect();
        f.write_str(&ids.join(","))
    }
}

impl std::str::FromStr for Version {
    type Err = VersionError;

    fn from_str(s: &str) -> Result<Version, VersionError> {
        if s == "-" {
            return Ok(Version(Frontiers::default()));
        }
        s.split(',')
            .map(|id| {
                let (counter, peer) = id.split_once('@')?;
                Some(ID::new(peer.parse().ok()?, counter.parse().ok()?))
            })
            .collect::<Option<Vec<ID>>>()
            .map(|ids| Version(Frontiers::from(ids)))
            .ok_or_else(|| VersionError(s.to_owned()))
    }
}

impl TryFrom<String> for Version {
    type Error = VersionError;

    fn try_from(s: String) -> Result<Version, VersionError> {
        s.parse()
    }
}

impl Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// The edits that one replica has and another lacks, as bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update(Vec<u8>);

impl Update {
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Update {
        Update(bytes)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// The bytes are not a snapshot of a document.
#[derive(Debug, thiserror::Error)]
#[error("the bytes are not a document snapshot: {0}")]
pub struct LoadError(String);

/// The bytes are not an update of a document.
#[derive(Debug, thiserror::Error)]
#[error("the bytes are not a document update: {0}")]
pub struct MergeError(String);

/// The version is not a version of this document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the version is not a version of this document")]
pub struct UnknownVersion;

/// The range of a splice ends after the text, which has `len`
/// characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the range ends after the text, which has {len} characters")]
pub struct SpliceError {
    pub len: usize,
}

/// Why [`Editor::replace`] changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReplaceError {
    #[error("the text to replace is empty")]
    Empty,
    #[error("the text to replace does not occur in the document")]
    NotFound,
    #[error("the text to replace occurs {0} times in the document")]
    Ambiguous(usize),
}

/// One run of lines that differs between two versions of the text.
///
/// Lines count from 0, and each line includes its line break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The line of the old text where `removed` starts.
    pub old_line: usize,
    pub removed: Vec<String>,
    /// The line of the new text where `added` starts.
    pub new_line: usize,
    pub added: Vec<String>,
    /// The writers of the characters that the edit adds. For an edit
    /// that adds no character, every writer that wrote between the two
    /// versions.
    pub by: Vec<Writer>,
}

impl Document {
    /// A replica of an empty document.
    #[must_use]
    pub fn new() -> Document {
        let doc = LoroDoc::new();
        doc.set_record_timestamp(true);
        Document { doc }
    }

    /// This replica, for `writer` to edit.
    #[must_use]
    pub fn editor(&mut self, writer: Writer) -> Editor<'_> {
        Editor {
            document: self,
            writer,
        }
    }

    /// A replica of the document in `snapshot`, which
    /// [`Document::save`] wrote.
    ///
    /// # Errors
    ///
    /// [`LoadError`] if the bytes are not a snapshot.
    ///
    /// # Panics
    ///
    /// If the snapshot contains an edit of no writer.
    pub fn load(snapshot: &[u8]) -> Result<Document, LoadError> {
        let document = Document::new();
        document
            .doc
            .import(snapshot)
            .map_err(|error| LoadError(error.to_string()))?;
        document.assert_writers();
        Ok(document)
    }

    /// The whole document with its history, as bytes for
    /// [`Document::load`].
    ///
    /// # Panics
    ///
    /// Never: Loro encodes every document.
    #[must_use]
    pub fn save(&self) -> Vec<u8> {
        self.doc
            .export(ExportMode::Snapshot)
            .expect("Loro encodes a snapshot of every document")
    }

    /// The Markdown text of the document.
    #[must_use]
    pub fn text(&self) -> String {
        self.content().to_string()
    }

    /// The edits that this replica contains.
    #[must_use]
    pub fn version(&self) -> Version {
        Version(self.doc.oplog_frontiers())
    }

    /// The edits that this replica has and a replica at `version` lacks.
    /// For a version with an edit that this replica lacks, the update
    /// contains every edit of this replica.
    ///
    /// # Panics
    ///
    /// Never: Loro encodes the updates of every document.
    #[must_use]
    pub fn updates_since(&self, version: &Version) -> Update {
        Update(
            self.doc
                .export(ExportMode::updates_owned(
                    self.vector(version).unwrap_or_default(),
                ))
                .expect("Loro encodes the updates of every document"),
        )
    }

    /// Takes in the edits of another replica. A merge of edits that this
    /// replica already has changes nothing.
    ///
    /// # Errors
    ///
    /// [`MergeError`] if the bytes are not an update.
    ///
    /// # Panics
    ///
    /// If the update contains an edit of no writer. Merge into a replica
    /// that is safe to drop.
    pub fn merge(&mut self, update: &Update) -> Result<(), MergeError> {
        self.doc
            .import(&update.0)
            .map_err(|error| MergeError(error.to_string()))?;
        self.assert_writers();
        Ok(())
    }

    /// The version that contains the edits of `writer` and the edits
    /// that those depend on, and no other edit.
    ///
    /// # Panics
    ///
    /// Never: the replica has every edit that it lists.
    #[must_use]
    pub fn last_edit_by(&self, writer: Writer) -> Version {
        let writers = self.writers();
        let ids: Vec<ID> = self
            .doc
            .oplog_vv()
            .iter()
            .filter(|(peer, counter)| **counter > 0 && writers.get(*peer) == Some(&writer))
            .map(|(peer, counter)| ID::new(*peer, counter - 1))
            .collect();
        let vector = self
            .doc
            .frontiers_to_vv(&Frontiers::from(ids))
            .expect("the replica has these edits");
        Version(self.doc.vv_to_frontiers(&vector))
    }

    /// Whether this replica has every edit of `version`.
    #[must_use]
    pub fn contains(&self, version: &Version) -> bool {
        self.vector(version).is_some()
    }

    /// The text of the document at `version`.
    ///
    /// # Errors
    ///
    /// [`UnknownVersion`] if this replica lacks an edit of `version`.
    ///
    /// # Panics
    ///
    /// Never: the document keeps its full history.
    pub fn text_at(&self, version: &Version) -> Result<String, UnknownVersion> {
        if !self.contains(version) {
            return Err(UnknownVersion);
        }
        let then = self
            .doc
            .fork_at(&version.0)
            .expect("the document keeps its full history");
        Ok(then.get_text(TEXT).to_string())
    }

    /// The runs of lines that differ from the text at `version`, in
    /// order. The edits, applied to that text, give the current text.
    ///
    /// # Errors
    ///
    /// [`UnknownVersion`] if this replica lacks an edit of `version`.
    pub fn edits_since(&self, version: &Version) -> Result<Vec<Edit>, UnknownVersion> {
        let old = self.text_at(version)?;
        let new = self.text();
        let seen = self.vector(version).ok_or(UnknownVersion)?;
        let writers = self.writers();
        let since: BTreeSet<Writer> = self
            .doc
            .oplog_vv()
            .iter()
            .filter(|(peer, end)| seen.get(peer).is_none_or(|known| known < *end))
            .filter_map(|(peer, _)| writers.get(peer).copied())
            .collect();
        let content = self.content();
        let writers_of = |from: usize, to: usize| -> BTreeSet<Writer> {
            (from..to)
                .filter_map(|at| content.get_cursor(at, Side::Middle)?.id)
                .filter(|id| seen.get(&id.peer).is_none_or(|known| *known <= id.counter))
                .filter_map(|id| writers.get(&id.peer).copied())
                .collect()
        };

        let diff = TextDiff::from_lines(&old, &new);
        let (old_lines, new_lines) = (diff.old_slices(), diff.new_slices());
        let mut starts = Vec::with_capacity(new_lines.len() + 1);
        let mut chars = 0;
        for line in new_lines {
            starts.push(chars);
            chars += line.chars().count();
        }
        starts.push(chars);

        let mut edits: Vec<Edit> = Vec::new();
        let mut adjoins = false;
        let (mut old_line, mut new_line) = (0, 0);
        for op in diff.ops() {
            let removed = old_line..old_line + op.old_range().len();
            let added = new_line..new_line + op.new_range().len();
            (old_line, new_line) = (removed.end, added.end);
            if op.tag() == DiffTag::Equal {
                adjoins = false;
                continue;
            }
            let by = writers_of(starts[added.start], starts[added.end]);
            let lines = |lines: &[&str]| -> Vec<String> {
                lines.iter().map(|line| (*line).to_owned()).collect()
            };
            match edits.last_mut().filter(|_| adjoins) {
                Some(edit) => {
                    edit.removed.extend(lines(&old_lines[removed]));
                    edit.added.extend(lines(&new_lines[added]));
                    let all: BTreeSet<Writer> = edit.by.iter().copied().chain(by).collect();
                    edit.by = all.into_iter().collect();
                }
                None => edits.push(Edit {
                    old_line: removed.start,
                    removed: lines(&old_lines[removed]),
                    new_line: added.start,
                    added: lines(&new_lines[added]),
                    by: by.into_iter().collect(),
                }),
            }
            adjoins = true;
        }
        for edit in &mut edits {
            if edit.by.is_empty() {
                edit.by = since.iter().copied().collect();
            }
        }
        Ok(edits)
    }
}

impl Document {
    #[cfg(test)]
    pub(crate) fn with_peer(self, peer: PeerID) -> Document {
        self.doc
            .set_peer_id(peer)
            .expect("the replica has no edits");
        self
    }

    fn content(&self) -> LoroText {
        self.doc.get_text(TEXT)
    }

    fn vector(&self, version: &Version) -> Option<VersionVector> {
        self.doc.frontiers_to_vv(&version.0)
    }

    fn writers(&self) -> HashMap<PeerID, Writer> {
        let LoroValue::Map(map) = self.doc.get_map(WRITERS).get_value() else {
            return HashMap::new();
        };
        map.iter()
            .filter_map(|(peer, writer)| {
                let LoroValue::String(writer) = writer else {
                    return None;
                };
                let writer = Writer::ALL.into_iter().find(|w| *w.as_str() == **writer)?;
                Some((peer.parse().ok()?, writer))
            })
            .collect()
    }

    fn assert_writers(&self) {
        let writers = self.writers();
        for (peer, counter) in self.doc.oplog_vv().iter() {
            assert!(
                *counter == 0 || writers.contains_key(peer),
                "the document records no writer for replica {peer}"
            );
        }
    }
}

impl Editor<'_> {
    /// Removes `delete` characters at `at` and inserts `insert` there.
    /// A position counts Unicode scalar values.
    ///
    /// # Errors
    ///
    /// [`SpliceError`] if the range ends after the text. The replica is
    /// then unchanged.
    ///
    /// # Panics
    ///
    /// If the text after the edit is not the text before it with the
    /// splice applied.
    pub fn splice(&mut self, at: usize, delete: usize, insert: &str) -> Result<(), SpliceError> {
        let content = self.document.content();
        let len = content.len_unicode();
        if at.checked_add(delete).is_none_or(|end| end > len) {
            return Err(SpliceError { len });
        }
        if delete == 0 && insert.is_empty() {
            return Ok(());
        }
        let before = content.to_string();
        content
            .splice(at, delete, insert)
            .expect("the range is inside the text");
        let doc = &self.document.doc;
        let (writers, peer) = (doc.get_map(WRITERS), doc.peer_id().to_string());
        if writers.get(&peer).is_none() {
            writers
                .insert(&peer, self.writer.as_str())
                .expect("a map accepts a value");
        }
        doc.commit();

        let mut expected: String = before.chars().take(at).collect();
        expected.push_str(insert);
        expected.extend(before.chars().skip(at + delete));
        assert_eq!(
            content.to_string(),
            expected,
            "a splice changes only its range"
        );
        self.document.assert_writers();
        Ok(())
    }

    /// Replaces `old`, which must occur exactly once in the text, with
    /// `new`. The edit is one splice of the characters that differ, so a
    /// concurrent edit of the characters that stay remains.
    ///
    /// # Errors
    ///
    /// [`ReplaceError`] if `old` is empty or does not occur exactly
    /// once. The replica is then unchanged.
    ///
    /// # Panics
    ///
    /// As [`Editor::splice`].
    pub fn replace(&mut self, old: &str, new: &str) -> Result<(), ReplaceError> {
        if old.is_empty() {
            return Err(ReplaceError::Empty);
        }
        let text = self.document.text();
        let mut found = text.match_indices(old);
        let at = match (found.next(), found.count()) {
            (None, _) => return Err(ReplaceError::NotFound),
            (Some((at, _)), 0) => at,
            (Some(_), more) => return Err(ReplaceError::Ambiguous(more + 1)),
        };
        self.splice_difference(text[..at].chars().count(), old, new);
        Ok(())
    }

    /// Replaces the whole text with `text`. The edit is one splice of
    /// the characters between the common prefix and the common suffix
    /// of the two texts, so a concurrent edit of the characters that
    /// stay remains. Equal texts make no edit.
    ///
    /// # Panics
    ///
    /// As [`Editor::splice`].
    pub fn set_text(&mut self, text: &str) {
        let old = self.document.text();
        self.splice_difference(0, &old, text);
    }

    /// Adds `text` at the end as a block of its own: one empty line
    /// separates it from a text before it, and one line break ends it.
    /// The line breaks at the end of the document and around `text` do
    /// not count. A `text` of line breaks only makes no edit.
    ///
    /// # Panics
    ///
    /// As [`Editor::splice`].
    pub fn append(&mut self, text: &str) {
        const BREAKS: [char; 2] = ['\n', '\r'];
        let block = text.trim_matches(BREAKS);
        if block.is_empty() {
            return;
        }
        let old = self.document.text();
        let kept = old.trim_end_matches(BREAKS);
        let gap = if kept.is_empty() { "" } else { "\n\n" };
        self.splice_difference(
            kept.chars().count(),
            &old[kept.len()..],
            &format!("{gap}{block}\n"),
        );
    }
}

impl Editor<'_> {
    fn splice_difference(&mut self, at: usize, old: &str, new: &str) {
        let (from, to): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
        let prefix = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
        let suffix = from[prefix..]
            .iter()
            .rev()
            .zip(to[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let insert: String = to[prefix..to.len() - suffix].iter().collect();
        self.splice(at + prefix, from.len() - prefix - suffix, &insert)
            .expect("the range is inside the text");
    }
}
