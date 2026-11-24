use crate::Document;

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
    }
}

#[test]
#[ignore = "run by hand when the stored format changes, with a new file number"]
fn write_snapshot() {
    use crate::Writer;
    use crate::tests::{fork, sync, written};

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
