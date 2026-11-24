mod properties;
mod reference;
mod stored;

use crate::{Document, Edit, Writer};

fn fork(base: &Document, peer: u64) -> Document {
    Document::load(&base.save())
        .expect("a saved document loads")
        .with_peer(peer)
}

fn sync(from: &Document, to: &mut Document) {
    to.merge(&from.updates_since(&to.version()))
        .expect("an update of a replica merges");
}

fn spliced(text: &str, at: usize, delete: usize, insert: &str) -> String {
    let mut result: String = text.chars().take(at).collect();
    result.push_str(insert);
    result.extend(text.chars().skip(at + delete));
    result
}

fn applied(old: &str, edits: &[Edit]) -> String {
    let lines: Vec<&str> = old.split_inclusive('\n').collect();
    let mut result = String::new();
    let mut next = 0;
    for edit in edits {
        result.extend(lines[next..edit.old_line].iter().copied());
        assert_eq!(lines[edit.old_line..][..edit.removed.len()], edit.removed);
        result.extend(edit.added.iter().map(String::as_str));
        next = edit.old_line + edit.removed.len();
    }
    result.extend(lines[next..].iter().copied());
    result
}

fn written(text: &str) -> Document {
    let mut document = Document::new().with_peer(1);
    document.editor(Writer::Author).splice(0, 0, text).unwrap();
    document
}
