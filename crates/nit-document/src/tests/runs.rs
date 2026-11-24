use nit_types::domain::LineKind;

use crate::tests::{fork, sync, written};
use crate::{Document, Version, Writer};

fn writers(document: &Document) -> Vec<Writer> {
    document.runs().iter().map(|run| run.writer).collect()
}

#[test]
fn a_document_with_no_edit_has_no_run() {
    assert_eq!(Document::new().runs(), Vec::new());
}

#[test]
fn consecutive_edits_of_one_writer_are_one_run() {
    let mut author = written("one\n");
    author.editor(Writer::Author).splice(4, 0, "two\n").unwrap();
    let mut reviewer = fork(&author, 2);
    reviewer
        .editor(Writer::Reviewer)
        .replace("two", "2")
        .unwrap();
    let mut second = fork(&reviewer, 3);
    second
        .editor(Writer::Author)
        .splice(0, 0, "zero\n")
        .unwrap();

    assert_eq!(
        writers(&second),
        [Writer::Author, Writer::Reviewer, Writer::Author]
    );
    assert_eq!(second.runs()[2].version, second.version());
    let texts: Vec<String> = second
        .runs()
        .iter()
        .map(|run| second.text_at(&run.version).unwrap())
        .collect();
    assert_eq!(texts, ["one\ntwo\n", "one\n2\n", "zero\none\n2\n"]);
}

#[test]
fn a_run_has_the_time_of_its_last_edit() {
    let mut author = Document::new().with_peer(1);
    for (seconds, text) in [(60, "one\n"), (120, "two\n")] {
        author.set_time_of_next_edit(seconds);
        author.editor(Writer::Author).splice(0, 0, text).unwrap();
    }
    let times: Vec<String> = author.runs().into_iter().map(|run| run.at).collect();
    assert_eq!(times, ["1970-01-01T00:02:00Z"]);
}

#[test]
fn concurrent_edits_of_two_writers_are_two_runs() {
    let mut author = written("one\n");
    let mut reviewer = fork(&author, 2);
    for n in 0..3 {
        author.editor(Writer::Author).splice(n, 0, "a").unwrap();
        reviewer.editor(Writer::Reviewer).splice(n, 0, "r").unwrap();
    }
    sync(&reviewer, &mut author);
    sync(&author, &mut reviewer);

    assert_eq!(writers(&author), [Writer::Author, Writer::Reviewer]);
    assert_eq!(author.runs(), reviewer.runs());
}

#[test]
fn the_lines_between_two_versions_are_the_whole_text_as_a_diff() {
    let mut author = written("one\ntwo\nthree\n");
    let from = author.version();
    author
        .editor(Writer::Author)
        .replace("two\n", "2\ntwo and a half\n")
        .unwrap();

    let lines: Vec<_> = author
        .lines_between(&from, &author.version())
        .unwrap()
        .into_iter()
        .map(|line| (line.kind, line.old, line.new, line.text))
        .collect();
    assert_eq!(
        lines,
        [
            (LineKind::Context, Some(1), Some(1), "one".to_owned()),
            (LineKind::Del, Some(2), None, "two".to_owned()),
            (LineKind::Add, None, Some(2), "2".to_owned()),
            (LineKind::Add, None, Some(3), "two and a half".to_owned()),
            (LineKind::Context, Some(3), Some(4), "three".to_owned()),
        ]
    );

    let empty: Version = "-".parse().unwrap();
    let added = author.lines_between(&empty, &from).unwrap();
    assert!(added.iter().all(|line| line.kind == LineKind::Add));
    assert_eq!(added.len(), 3);
    let unknown: Version = "4@9".parse().unwrap();
    assert!(author.lines_between(&from, &unknown).is_err());
}
