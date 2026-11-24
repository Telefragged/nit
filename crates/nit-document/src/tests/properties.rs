use proptest::prelude::*;
use proptest::sample::Index;

use crate::tests::{applied, fork, spliced, sync, written};
use crate::{Document, ReplaceError, SpliceError, Update, Writer, references};

const REPLICAS: usize = 3;

#[derive(Debug, Clone)]
struct Splice {
    at: Index,
    delete: Index,
    insert: String,
}

#[derive(Debug, Clone)]
enum Step {
    Edit { replica: usize, splice: Splice },
    Sync { from: usize, to: usize },
    Capture { from: usize, to: usize },
    Deliver { update: Index, to: usize },
}

fn text() -> impl Strategy<Value = String> {
    "[a-cé\\n ]{0,6}"
}

fn wide_text() -> impl Strategy<Value = String> {
    "[ab\\n😀𝄞]{0,8}"
}

fn splice() -> impl Strategy<Value = Splice> {
    (any::<Index>(), any::<Index>(), text()).prop_map(|(at, delete, insert)| Splice {
        at,
        delete,
        insert,
    })
}

fn step() -> impl Strategy<Value = Step> {
    let replica = || 0..REPLICAS;
    prop_oneof![
        6 => (replica(), splice()).prop_map(|(replica, splice)| Step::Edit { replica, splice }),
        2 => (replica(), replica()).prop_map(|(from, to)| Step::Sync { from, to }),
        1 => (replica(), replica()).prop_map(|(from, to)| Step::Capture { from, to }),
        1 => (any::<Index>(), replica()).prop_map(|(update, to)| Step::Deliver { update, to }),
    ]
}

fn range(text: &str, splice: &Splice) -> (usize, usize) {
    let len = text.chars().count();
    let at = splice.at.index(len + 1);
    (at, splice.delete.index((len - at).min(4) + 1))
}

fn writer(replica: usize) -> Writer {
    if replica == 0 {
        Writer::Author
    } else {
        Writer::Reviewer
    }
}

fn replicas() -> Vec<Document> {
    let base = Document::new();
    (0..REPLICAS)
        .map(|n| fork(&base, u64::try_from(n).unwrap() + 1))
        .collect()
}

fn pair(replicas: &mut [Document], from: usize, to: usize) -> (&Document, &mut Document) {
    if from < to {
        let (left, right) = replicas.split_at_mut(to);
        (&left[from], &mut right[0])
    } else {
        let (left, right) = replicas.split_at_mut(from);
        (&right[0], &mut left[to])
    }
}

fn edits_give_the_text(replica: &Document, version: &crate::Version) -> Result<(), TestCaseError> {
    let edits = replica.edits_since(version).unwrap();
    let then = replica.text_at(version).unwrap();
    prop_assert_eq!(applied(&then, &edits), replica.text());
    for edit in &edits {
        prop_assert!(!edit.by.is_empty(), "{:?} has no writer", edit);
    }
    Ok(())
}

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 512,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn replicas_converge(steps in proptest::collection::vec(step(), 1..48)) {
        let mut replicas = replicas();
        let mut kept: Vec<Update> = Vec::new();
        let half = steps.len() / 2;
        let mut then = None;
        for (n, step) in steps.iter().enumerate() {
            if n == half {
                then = Some(replicas[0].version());
            }
            match step {
                Step::Edit { replica, splice } => {
                    let before = replicas[*replica].text();
                    let (at, delete) = range(&before, splice);
                    replicas[*replica]
                        .editor(writer(*replica))
                        .splice(at, delete, &splice.insert)
                        .unwrap();
                    prop_assert_eq!(
                        replicas[*replica].text(),
                        spliced(&before, at, delete, &splice.insert)
                    );
                }
                Step::Sync { from, to } if from != to => {
                    let (from, to) = pair(&mut replicas, *from, *to);
                    sync(from, to);
                }
                Step::Sync { .. } => {}
                Step::Capture { from, to } => {
                    kept.push(replicas[*from].updates_since(&replicas[*to].version()));
                }
                Step::Deliver { update, to } => {
                    if !kept.is_empty() {
                        let update = &kept[update.index(kept.len())];
                        let replica = &mut replicas[*to];
                        let before = replica.version();
                        replica.merge(update).unwrap();
                        let once = (replica.text(), replica.version());
                        replica.merge(update).unwrap();
                        prop_assert_eq!(once, (replica.text(), replica.version()));
                        edits_give_the_text(replica, &before)?;
                    }
                }
            }
        }

        for _ in 0..2 {
            for from in 0..REPLICAS {
                for to in 0..REPLICAS {
                    if from != to {
                        let (from, to) = pair(&mut replicas, from, to);
                        sync(from, to);
                    }
                }
            }
        }
        for replica in &replicas[1..] {
            prop_assert_eq!(replica.text(), replicas[0].text());
            prop_assert_eq!(replica.version(), replicas[0].version());
        }

        let reloaded = Document::load(&replicas[0].save()).unwrap();
        prop_assert_eq!(reloaded.text(), replicas[0].text());
        prop_assert_eq!(reloaded.version(), replicas[0].version());

        if let Some(version) = then {
            edits_give_the_text(&replicas[0], &version)?;
        }
    }

    #[test]
    fn one_replica_matches_a_string(splices in proptest::collection::vec(splice(), 1..64)) {
        let mut replica = Document::new().with_peer(1);
        let mut model = String::new();
        for splice in &splices {
            let (at, delete) = range(&model, splice);
            replica.editor(Writer::Author).splice(at, delete, &splice.insert).unwrap();
            model = spliced(&model, at, delete, &splice.insert);
            prop_assert_eq!(&replica.text(), &model);
        }
    }

    #[test]
    fn two_concurrent_splices_both_remain(
        base in "[a-c\\n ]{0,12}",
        first in (any::<Index>(), any::<Index>(), "X{1,4}"),
        second in (any::<Index>(), any::<Index>(), "Y{1,4}"),
    ) {
        let author = written(&base);
        let mut replicas = [fork(&author, 2), fork(&author, 3)];
        let mut removed = std::collections::BTreeSet::new();
        for (replica, (at, delete, insert)) in replicas.iter_mut().zip([&first, &second]) {
            let splice = Splice { at: *at, delete: *delete, insert: insert.clone() };
            let (at, delete) = range(&base, &splice);
            replica.editor(Writer::Reviewer).splice(at, delete, insert).unwrap();
            removed.extend(at..at + delete);
        }
        let (from, to) = pair(&mut replicas, 0, 1);
        sync(from, to);
        let merged = replicas[1].text();
        prop_assert!(merged.contains(&first.2), "{:?} lacks {:?}", merged, first.2);
        prop_assert!(merged.contains(&second.2), "{:?} lacks {:?}", merged, second.2);
        let kept: String = base
            .chars()
            .enumerate()
            .filter(|(n, _)| !removed.contains(n))
            .map(|(_, c)| c)
            .collect();
        let rest: String = merged.chars().filter(|c| !matches!(c, 'X' | 'Y')).collect();
        prop_assert_eq!(rest, kept);
    }

    #[test]
    fn replace_changes_the_one_occurrence(
        text in "[ab\\n]{0,10}",
        old in "[ab\\n]{0,3}",
        new in "[abc\\n]{0,3}",
    ) {
        let mut replica = written(&text);
        let version = replica.version();
        let result = replica.editor(Writer::Author).replace(&old, &new);
        let occurrences = if old.is_empty() { 0 } else { text.matches(&old).count() };
        match result {
            Ok(()) => {
                prop_assert_eq!(occurrences, 1);
                prop_assert_eq!(replica.text(), text.replacen(&old, &new, 1));
            }
            Err(error) => {
                let expected = match occurrences {
                    0 if old.is_empty() => ReplaceError::Empty,
                    0 => ReplaceError::NotFound,
                    n => ReplaceError::Ambiguous(n),
                };
                prop_assert_eq!(error, expected);
                prop_assert_eq!(replica.text(), text);
                prop_assert_eq!(replica.version(), version);
            }
        }
    }

    #[test]
    fn every_text_has_references(text in "[a-z \\n\\[\\]()!/?=&#*>`0-9-]{0,80}") {
        let lines = text.split('\n').count();
        for placed in references(&text) {
            prop_assert!(placed.line < lines);
            prop_assert_eq!(placed.reference.to_string().parse(), Ok(placed.reference));
        }
    }

    #[test]
    fn set_text_gives_the_text(old in wide_text(), new in wide_text()) {
        let mut replica = written(&old);
        replica.editor(Writer::Author).set_text(&new);
        prop_assert_eq!(&replica.text(), &new);
        let version = replica.version();
        replica.editor(Writer::Author).set_text(&new);
        prop_assert_eq!(replica.text(), new);
        prop_assert_eq!(replica.version(), version);
    }

    #[test]
    fn two_concurrent_set_texts_of_different_parts_both_remain(
        (head, tail) in (wide_text(), wide_text()),
        (new_head, new_tail) in (wide_text(), wide_text()),
    ) {
        let author = written(&format!("{head}|{tail}"));
        let mut replicas = [fork(&author, 2), fork(&author, 3)];
        replicas[0].editor(Writer::Reviewer).set_text(&format!("{new_head}|{tail}"));
        replicas[1].editor(Writer::Reviewer).set_text(&format!("{head}|{new_tail}"));
        let (from, to) = pair(&mut replicas, 0, 1);
        sync(from, to);
        prop_assert_eq!(replicas[1].text(), format!("{new_head}|{new_tail}"));
    }
}

#[test]
fn append_adds_a_block_after_one_empty_line() {
    for (before, text, after) in [
        ("", "one", "one\n"),
        ("", "\none\n\n", "one\n"),
        ("\n\n", "one", "one\n"),
        ("one", "two", "one\n\ntwo\n"),
        ("one\n", "two\n", "one\n\ntwo\n"),
        ("one\n\n\n", "\ntwo\nthree", "one\n\ntwo\nthree\n"),
        ("one\r\n", "two\r\n", "one\n\ntwo\n"),
    ] {
        let mut replica = written(before);
        replica.editor(Writer::Author).append(text);
        assert_eq!(replica.text(), after, "{before:?} with {text:?}");
    }
    let mut replica = written("one\n");
    let version = replica.version();
    replica.editor(Writer::Author).append("\n");
    assert_eq!(
        (replica.text(), replica.version()),
        ("one\n".into(), version)
    );
}

#[test]
fn a_splice_after_the_text_changes_nothing() {
    let mut replica = written("one");
    let version = replica.version();
    let result = replica.editor(Writer::Author).splice(2, 2, "x");
    assert_eq!(result, Err(SpliceError { len: 3 }));
    assert_eq!((replica.text(), replica.version()), ("one".into(), version));
}

#[test]
fn an_edit_has_the_writer_of_its_lines() {
    let mut author = written("one\ntwo\nthree\n");
    let version = author.version();
    let mut reviewer = fork(&author, 2);
    reviewer
        .editor(Writer::Reviewer)
        .replace("two", "2")
        .unwrap();
    author
        .editor(Writer::Author)
        .replace("three\n", "")
        .unwrap();
    sync(&reviewer, &mut author);

    let edits = author.edits_since(&version).unwrap();
    assert_eq!(author.text(), "one\n2\n");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].old_line, 1);
    assert_eq!(edits[0].removed, ["two\n", "three\n"]);
    assert_eq!(edits[0].added, ["2\n"]);
    assert_eq!(edits[0].by, [Writer::Reviewer]);

    let before = author.version();
    author.editor(Writer::Author).replace("one\n", "").unwrap();
    let edits = author.edits_since(&before).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].by, [Writer::Author]);
}

#[test]
fn the_edits_since_the_last_edit_of_the_author_are_of_the_reviewer() {
    let mut author = written("one\ntwo\n");
    let mut reviewer = fork(&author, 2);
    reviewer
        .editor(Writer::Reviewer)
        .replace("two", "2")
        .unwrap();
    author.editor(Writer::Author).replace("one", "1").unwrap();
    sync(&author, &mut reviewer);
    reviewer
        .editor(Writer::Reviewer)
        .replace("2\n", "2\nthree\n")
        .unwrap();
    sync(&reviewer, &mut author);

    let edits = author
        .edits_since(&author.last_edit_by(Writer::Author))
        .unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].old_line, 1);
    assert_eq!(edits[0].removed, ["two\n"]);
    assert_eq!(edits[0].added, ["2\n", "three\n"]);
    assert_eq!(edits[0].by, [Writer::Reviewer]);
    assert_eq!(author.last_edit_by(Writer::Reviewer), author.version());
}

#[test]
fn a_version_of_another_document_is_unknown() {
    let other = written("other");
    let document = Document::new();
    assert!(document.text_at(&other.version()).is_err());
    assert!(document.edits_since(&other.version()).is_err());
}
