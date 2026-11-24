use nit_types::domain::{AttachmentHash, ChangeNumber, RevisionNumber, Side};
use proptest::prelude::*;

use crate::{LineRange, PlacedReference, Reference, references};

fn name() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 _.()é&=+%#?*-]{1,12}"
}

fn lines() -> impl Strategy<Value = LineRange> {
    (any::<bool>(), 1..500u64, 0..40u64).prop_map(|(old, start, more)| {
        let side = if old { Side::Old } else { Side::New };
        LineRange::new(side, start, start + more).unwrap()
    })
}

fn reference() -> impl Strategy<Value = Reference> {
    let change = || (0..5000u64).prop_map(ChangeNumber::new);
    let revision = || (0..40u64).prop_map(RevisionNumber::new);
    prop_oneof![
        change().prop_map(|change| Reference::Change { change }),
        (
            change(),
            revision(),
            proptest::option::of(revision()),
            proptest::collection::vec(name(), 1..4),
            proptest::option::of(lines()),
        )
            .prop_map(|(change, revision, against, path, lines)| Reference::File {
                change,
                revision,
                against,
                file: path.join("/"),
                lines,
            }),
        ("[0-9a-f]{64}", name()).prop_map(|(hash, name)| Reference::Upload {
            hash: AttachmentHash::new(hash).unwrap(),
            name,
        }),
    ]
}

proptest! {
    #[test]
    fn an_address_parses_to_its_reference(reference in reference()) {
        let address = reference.to_string();
        prop_assert_eq!(address.parse::<Reference>(), Ok(reference.clone()));
        let absolute = format!("http://127.0.0.1:8877{address}");
        prop_assert_eq!(absolute.parse::<Reference>(), Ok(reference));
    }

    #[test]
    fn a_link_on_its_own_line_is_a_reference(
        first in reference(),
        second in reference(),
        image in any::<bool>(),
    ) {
        let mark = if image { "!" } else { "" };
        let text = format!("# Title\n\nSee [it]({first}) here.\n\n{mark}[label]({second})\n\nEnd.\n");
        prop_assert_eq!(references(&text), [PlacedReference { line: 4, reference: second }]);
    }
}

#[test]
fn only_a_whole_top_level_paragraph_is_a_reference() {
    let text = "\
[one](/changes/1)

- [in a list](/changes/2)

> [in a quote](/changes/3)

[two](/changes/4) and more

[not nit](https://example.com/page)

```
[code](/changes/5)
```

[file](/changes/6?revision=2&file=src/a.rs&line=3)

http://127.0.0.1:8877/changes/7?revision=0&file=a_b.rs

http://127.0.0.1:8877/changes/8 with text
";
    assert_eq!(
        references(text),
        [
            PlacedReference {
                line: 0,
                reference: Reference::Change {
                    change: ChangeNumber::new(1)
                },
            },
            PlacedReference {
                line: 14,
                reference: Reference::File {
                    change: ChangeNumber::new(6),
                    revision: RevisionNumber::new(2),
                    against: None,
                    file: "src/a.rs".into(),
                    lines: Some(LineRange::new(Side::New, 3, 3).unwrap()),
                },
            },
            PlacedReference {
                line: 16,
                reference: Reference::File {
                    change: ChangeNumber::new(7),
                    revision: RevisionNumber::new(0),
                    against: None,
                    file: "a_b.rs".into(),
                    lines: None,
                },
            },
        ]
    );
}

#[test]
fn a_file_address_requires_a_revision() {
    assert!("/changes/6?file=src/a.rs".parse::<Reference>().is_err());
    assert!("/changes/six".parse::<Reference>().is_err());
    assert!(
        "/api/attachments/abc/name.png"
            .parse::<Reference>()
            .is_err()
    );
}
