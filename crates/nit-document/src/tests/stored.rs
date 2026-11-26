use std::fmt::Write;

use crate::tests::{fork, sync, written};
use crate::{Document, Writer};

const STORED: &[(&[u8], &str)] = &[(
    include_bytes!("../../tests/snapshots/1.snapshot"),
    include_str!("../../tests/snapshots/1.md"),
)];

#[test]
fn stored_snapshots_load() {
    for (snapshot, text) in STORED {
        let document = Document::load(snapshot).unwrap();
        assert_eq!(document.text(), *text);
        assert_ne!(
            document.edits_since(&"-".parse().unwrap()).unwrap(),
            Vec::new()
        );
        let runs = document.runs();
        let writers: Vec<Writer> = runs.iter().map(|run| run.writer).collect();
        assert_eq!(writers, [Writer::Author, Writer::Reviewer]);
        assert_eq!(runs[1].version, document.version());
    }
}

#[test]
#[ignore = "run by hand when the stored format changes, with a new file number"]
fn write_snapshot() {
    let mut author = written(
        "# Title\n\nFirst paragraph.\n\n[fold.rs](/changes/7?revision=1&file=src%2Ffold.rs&line=4-9)\n",
    );
    let mut reviewer = fork(&author, 2);
    reviewer
        .editor(Writer::Reviewer)
        .replace("# Title", "# Title of the plan")
        .unwrap();
    author
        .editor(Writer::Author)
        .replace("First paragraph.", "The first paragraph.\n\n- [ ] a task")
        .unwrap();
    sync(&reviewer, &mut author);

    let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots");
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(format!("{directory}/1.snapshot"), author.save()).unwrap();
    std::fs::write(format!("{directory}/1.md"), author.text()).unwrap();
}

const ROTATE: &str = "/changes/11?revision=1&file=src%2Fauth%2Frotate.rs&line=20-28";
const ROTATE_BEFORE: &str = "/changes/11?revision=0&file=src%2Fauth%2Frotate.rs&line=20-21";
const TEST: &str = "/changes/11?revision=1&file=tests%2Frotation.rs";
const STORE: &str = "/changes/11?revision=1&against=r0&file=src%2Fauth%2Fstore.rs";
const ATTACHED: &str = "/changes/12?revision=0&file=%2Fattachments%2F";

#[test]
#[ignore = "run by hand to change the document of the web fixtures"]
fn write_web_fixture() {
    const NINE: i64 = 1_767_258_000;
    let at = |replica: Document, minutes: i64| {
        replica.set_time_of_next_edit(NINE + minutes * 60);
        replica
    };
    let append = |replica: &mut Document, writer: Writer, text: &str| {
        let end = replica.text().chars().count();
        replica.editor(writer).splice(end, 0, text).unwrap();
    };

    let mut author = at(Document::new().with_peer(1), 0);
    append(
        &mut author,
        Writer::Author,
        &format!(
            "Three changes add rotation of refresh tokens.\n\n[change 10](/changes/10)\n\n[change 11](/changes/11)\n\n## Reuse revokes the family\n\n`rotate` looks up the presented token. A token that was rotated before is a sign of theft, so the whole family is revoked.\n\n[rotate.rs]({ROTATE})\n"
        ),
    );
    let mut author = at(fork(&author, 3), 20);
    append(
        &mut author,
        Writer::Author,
        &format!(
            "\n## The store after the review\n\nThe interdiff from revision 0 to revision 1 shows what the review changed:\n\n- The lookup trims the token.\n\n[store.rs]({STORE})\n"
        ),
    );

    let mut reviewer = at(fork(&author, 2), 90);
    reviewer
        .editor(Writer::Reviewer)
        .replace(
            "refresh tokens.",
            "refresh tokens. Read them in this order: the schema, the rotation, and then the documentation.",
        )
        .unwrap();
    reviewer.set_time_of_next_edit(NINE + 90 * 60);
    reviewer
        .editor(Writer::Reviewer)
        .replace(
            "line=20-28)\n",
            &format!(
                "line=20-28)\n\nRevision 0 trusted the lookup. Revision 1 rewrote these lines, and [the test]({TEST}) covers the reuse.\n\n[rotate.rs at revision 0]({ROTATE_BEFORE})\n"
            ),
        )
        .unwrap();

    let mut author = at(fork(&reviewer, 4), 135);
    author
        .editor(Writer::Author)
        .replace(
            "- The lookup trims the token.\n",
            "- The lookup trims the presented token.\n- A rebase renamed the parameter of `insert`.\n",
        )
        .unwrap();
    author.set_time_of_next_edit(NINE + 135 * 60);
    append(
        &mut author,
        Writer::Author,
        &format!(
            "\n## The flow and the plan\n\n[flow.svg]({ATTACHED}web%2Fscreenshots%2Fflow.svg)\n\n[rotation.md]({ATTACHED}plan%2Frotation.md)\n\n<details>\n<summary>How the store finds a family</summary>\n\nEach token row has a `family_id`. One statement revokes every row of the family.\n\n</details>\n"
        ),
    );

    let mut reviewer = at(fork(&author, 5), 160);
    append(
        &mut reviewer,
        Writer::Reviewer,
        &format!(
            "\n[nit-log.txt](/api/attachments/{}/nit-log.txt)\n",
            "a".repeat(64)
        ),
    );

    for (old, new) in [
        ("looks up the presented token", "finds the presented token"),
        ("so the whole family is revoked", "so the family is revoked"),
        ("- A rebase renamed the parameter of `insert`.\n", ""),
    ] {
        reviewer.set_time_of_next_edit(NINE + 160 * 60);
        reviewer.editor(Writer::Reviewer).replace(old, new).unwrap();
    }

    let mut snapshot = String::new();
    for byte in reviewer.save() {
        write!(snapshot, "{byte:02x}").unwrap();
    }
    std::fs::write(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/src/api/fixtures/rotationPlan.json"
        ),
        format!("{{\n  \"snapshot\": \"{snapshot}\"\n}}\n"),
    )
    .unwrap();
}
