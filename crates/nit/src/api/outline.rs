//! Collapsing a file to its outline.
//!
//! `nit_types::domain` defines what an outline is. Here is the one place
//! that decides, per language, which spans a collapse drops.

use std::sync::LazyLock;

use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

/// Captures the body of each **named** definition, and each import.
///
/// What no pattern captures survives, which is what leaves a trait, an
/// interface and their bodiless signatures whole. A trait method with a
/// default body is a definition like any other: its signature stays, its
/// body goes.
///
/// A `pub use` is not an import but a re-export, so the anchor before
/// `use` keeps it: what a module hands on is its API surface.
const RUST_PATTERNS: &str = "
    (function_item body: (block) @delimited)
    (use_declaration . \"use\") @whole
";

/// A definition bound to a name, whether it declares one or is assigned to
/// one — the second form is how most of a React codebase is written.
/// A callback passed inline is not a definition and keeps its body.
///
/// An `export ... from` is a re-export rather than an import, and parses as
/// an export, so it survives with the rest of the API surface.
const TYPESCRIPT_PATTERNS: &str = "
    (function_declaration body: (statement_block) @delimited)
    (method_definition body: (statement_block) @delimited)
    (variable_declarator value: (arrow_function body: (statement_block) @delimited))
    (variable_declarator value: (function_expression body: (statement_block) @delimited))
    (import_statement) @whole
";

/// A `def` at any depth, so a method reads like a free function. A `lambda`
/// is not a definition and keeps its body, as an inline callback does.
const PYTHON_PATTERNS: &str = "
    (function_definition body: (block) @bare)
    (import_statement) @whole
    (import_from_statement) @whole
    (future_import_statement) @whole
";

/// What a Nix file exposes is its attribute paths, so an attrset is
/// structure and stays whole; a value built any other way is the body under
/// a path. A call, a list and an indented string are the three that run
/// long, and each carries its own brackets — so the path that opens one and
/// the bracket that closes it survive.
const NIX_PATTERNS: &str = "
    (apply_expression) @delimited
    (list_expression) @delimited
    (indented_string_expression) @delimited
";

/// The file's lines that its outline keeps, each with the 1-based number it
/// holds in the file.
///
/// Collapsing only ever removes lines, so what comes back is a subsequence
/// of the file carrying the numbers a reader would count to. A path in a
/// language with no grammar keeps every line, so the outline of a diff over
/// it is the diff itself.
pub(super) fn outline<'a>(path: &str, text: &'a str) -> (Vec<u64>, Vec<&'a str>) {
    let collapsed = collapsed_lines(path, text);
    text.lines()
        .enumerate()
        .filter(|(i, _)| !collapsed[*i])
        .map(|(i, line)| (i as u64 + 1, line))
        .unzip()
}

/// Which of the file's lines a collapse drops, indexed from 0.
///
/// A capture names how much of the span it holds goes. A `@delimited`
/// body's own first and last lines are not in it: the line it opens on ends
/// the signature, and the one it closes on carries the delimiter that shows
/// the signature was for a definition — so such a body has to span three
/// lines before collapsing hides anything. A `@bare` body is marked by its
/// indentation alone, so every line it covers goes, bar the one it shares
/// with the signature that introduces it. A `@whole` span is not a body at
/// all and nothing in it survives.
///
/// All false when the language has no grammar or its parse fails.
fn collapsed_lines(path: &str, text: &str) -> Vec<bool> {
    let mut collapsed = vec![false; text.lines().count()];
    let Some(grammar) = grammar(path) else {
        return collapsed;
    };
    let mut parser = Parser::new();
    if parser.set_language(&grammar.language).is_err() {
        return collapsed;
    }
    let Some(tree) = parser.parse(text, None) else {
        return collapsed;
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&grammar.query, tree.root_node(), text.as_bytes());
    while let Some(matched) = matches.next() {
        for span in matched.captures {
            let (first, last) = (span.node.start_position().row, span.node.end_position().row);
            let (opens, closes) = match grammar.query.capture_names()[span.index as usize] {
                "whole" => (first, last + 1),
                "bare" => {
                    let on_the_signature_line = span
                        .node
                        .parent()
                        .is_some_and(|def| def.start_position().row == first);
                    (first + usize::from(on_the_signature_line), last + 1)
                }
                _ => (first + 1, last),
            };
            if opens < closes {
                collapsed[opens..closes].fill(true);
            }
        }
    }
    collapsed
}

/// One language nit can outline, with its patterns compiled.
struct Grammar {
    language: Language,
    query: Query,
}

impl Grammar {
    /// Panics on patterns that do not compile, which
    /// `every_grammar_compiles_its_patterns` is what keeps out of a release.
    fn new(language: Language, patterns: &str) -> Self {
        let query = Query::new(&language, patterns).expect("outline patterns compile");
        Grammar { language, query }
    }
}

static RUST: LazyLock<Grammar> =
    LazyLock::new(|| Grammar::new(tree_sitter_rust::LANGUAGE.into(), RUST_PATTERNS));

static TYPESCRIPT: LazyLock<Grammar> = LazyLock::new(|| {
    Grammar::new(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        TYPESCRIPT_PATTERNS,
    )
});

/// TSX parses JSX where TypeScript parses the same characters as
/// comparisons, so the two are separate grammars over shared patterns.
static TSX: LazyLock<Grammar> = LazyLock::new(|| {
    Grammar::new(
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        TYPESCRIPT_PATTERNS,
    )
});

static PYTHON: LazyLock<Grammar> =
    LazyLock::new(|| Grammar::new(tree_sitter_python::LANGUAGE.into(), PYTHON_PATTERNS));

static NIX: LazyLock<Grammar> =
    LazyLock::new(|| Grammar::new(tree_sitter_nix::LANGUAGE.into(), NIX_PATTERNS));

/// The grammar for the language `path` is written in.
fn grammar(path: &str) -> Option<&'static Grammar> {
    match path.rsplit_once('.')?.1 {
        "rs" => Some(&RUST),
        "ts" | "mts" | "cts" => Some(&TYPESCRIPT),
        "tsx" => Some(&TSX),
        "py" | "pyi" => Some(&PYTHON),
        "nix" => Some(&NIX),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(path: &str, text: &str) -> String {
        let (at, lines) = outline(path, text);
        at.into_iter()
            .zip(lines)
            .map(|(at, line)| format!("{at}:{line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_grammar_compiles_its_patterns() {
        for grammar in [&RUST, &TYPESCRIPT, &TSX, &PYTHON, &NIX] {
            LazyLock::force(grammar);
        }
    }

    #[test]
    fn a_rust_body_goes_and_its_documented_signature_stays() {
        let text = "\
/// Adds.
pub fn add(a: u8, b: u8) -> u8 {
    let sum = a + b;
    sum
}
";
        assert_eq!(
            kept("m.rs", text),
            "1:/// Adds.\n2:pub fn add(a: u8, b: u8) -> u8 {\n5:}"
        );
    }

    #[test]
    fn a_trait_keeps_its_signatures_and_loses_its_default_bodies() {
        let text = "\
trait Tip {
    fn sha(&self) -> Sha;
    fn short(&self) -> String {
        self.sha().to_string()
    }
}
";
        assert_eq!(
            kept("m.rs", text),
            "1:trait Tip {\n2:    fn sha(&self) -> Sha;\n3:    fn short(&self) -> String {\n5:    }\n6:}"
        );
    }

    #[test]
    fn rust_imports_go_and_re_exports_stay() {
        let text = "\
use std::sync::{
    Arc, Mutex,
};
pub use crate::api::Diff;

struct Rev;
";
        assert_eq!(
            kept("m.rs", text),
            "4:pub use crate::api::Diff;\n5:\n6:struct Rev;"
        );
    }

    #[test]
    fn typescript_imports_go_and_re_exports_stay() {
        let text = "\
import { render } from \"react\";
export { Diff } from \"./diff\";
const id = 1;
";
        assert_eq!(
            kept("m.ts", text),
            "2:export { Diff } from \"./diff\";\n3:const id = 1;"
        );
    }

    #[test]
    fn python_imports_go_however_they_are_written() {
        let text = "\
from __future__ import annotations
import os
from typing import (
    Iterator,
)

class Chain:
    pass
";
        assert_eq!(kept("m.py", text), "6:\n7:class Chain:\n8:    pass");
    }

    #[test]
    fn a_struct_keeps_every_field() {
        let text = "struct Rev {\n    sha: Sha,\n    parent: Sha,\n}\n";
        assert_eq!(
            kept("m.rs", text),
            "1:struct Rev {\n2:    sha: Sha,\n3:    parent: Sha,\n4:}"
        );
    }

    #[test]
    fn a_named_arrow_collapses_and_an_inline_callback_does_not() {
        let text = "\
const load = async (id: number) => {
  const rows = await fetch(id);
  return rows;
};
items.filter((row) => {
  return row.open;
});
";
        assert_eq!(
            kept("m.ts", text),
            "1:const load = async (id: number) => {\n4:};\n5:items.filter((row) => {\n6:  return row.open;\n7:});"
        );
    }

    #[test]
    fn a_python_class_keeps_its_methods_and_loses_their_bodies() {
        let text = "\
class Chain:
    \"\"\"A chain.\"\"\"

    @property
    def tip(self) -> str:
        head = self.revs[-1]
        return head.sha
";
        assert_eq!(
            kept("m.py", text),
            "1:class Chain:\n2:    \"\"\"A chain.\"\"\"\n3:\n4:    @property\n5:    def tip(self) -> str:"
        );
    }

    #[test]
    fn a_python_body_on_the_signature_line_hides_nothing() {
        let text = "def nil(): pass\ndef one():\n    return 1\n";
        assert_eq!(kept("m.py", text), "1:def nil(): pass\n2:def one():");
    }

    #[test]
    fn a_nix_path_stays_and_the_derivation_under_it_goes() {
        let text = "\
{
  packages.default = pkgs.mkShell {
    packages = [ pkgs.git ];
  };
}
";
        assert_eq!(
            kept("m.nix", text),
            "1:{\n2:  packages.default = pkgs.mkShell {\n4:  };\n5:}"
        );
    }

    #[test]
    fn a_nix_list_and_script_keep_their_brackets() {
        let text = "\
{
  systems = [
    \"x86_64-linux\"
  ];
  shellHook = ''
    export NIT=1
  '';
}
";
        assert_eq!(
            kept("m.nix", text),
            "1:{\n2:  systems = [\n4:  ];\n5:  shellHook = ''\n7:  '';\n8:}"
        );
    }

    #[test]
    fn a_language_with_no_grammar_keeps_every_line() {
        let text = "a\nb\nc\n";
        assert_eq!(kept("m.hs", text), "1:a\n2:b\n3:c");
        assert_eq!(kept("LICENSE", text), "1:a\n2:b\n3:c");
    }

    #[test]
    fn a_rust_body_on_the_signature_line_hides_nothing() {
        let text = "fn nil() {}\nfn one() -> u8 {\n    1\n}\n";
        assert_eq!(kept("m.rs", text), "1:fn nil() {}\n2:fn one() -> u8 {\n4:}");
    }
}
