# Documents

A document is one Markdown text that embeds references to changes. Authors write documents to explain sets of changes, or to propose work before the changes exist. Reviewers edit the same text in the browser, at `/documents/<number>`.

The model is `crates/nit-document`. Its crate docs define the terms: document, reference, writer, edit.

## The layout of the crate

The layout of the crate lets a reference in a document show one public item.

- `lib.rs` is the list of the public API. Each module is private, and `lib.rs` exports each public item by its identifier. `wire` is the one public module, because it contains only the shapes of the HTTP API.
- The first `impl` block of a type contains its public methods, and nothing else. A second `impl` block contains the private and the test-only methods.
- A public item has a doc-comment. A type with a text form or a validated constructor has an example in it.
- A test uses the public API only, and is in `src/tests`.

## How documents are stored

The server stores each document as one Loro CRDT snapshot, in the `documents` table. The snapshot contains every edit with its writer and its time.

Each client has its own replica: the CLI for authors, and the browser, through `nit-wasm`, for reviewers. A client edits the text of its replica with an `Editor`, and posts the update bytes to `POST /api/documents/{number}/updates`. That route is the only one that changes the text. The server merges the update into a copy of the stored snapshot, and then stores the copy.

On each write, the server finds the references of the text again with `references`. The `document_attachments` table contains the hash of each upload, so the collection of attachments keeps that blob.

## References

A reference is a link or an image that is the whole of a top-level paragraph, with the address of a part of a change or of an upload. `Reference` parses and prints the address. `references` finds the references of a text, each with the first line of its paragraph, and the server, the CLI and the browser all use that one function. Every other text is ordinary Markdown, so every text is a valid document.

## The rules of a merge

Every edit is a splice of the text. Two replicas that have the same edits have the same text, and two concurrent splices that remove no common character both remain. Each rule is a property in `crates/nit-document/src/tests`.

- `tests/properties.rs` generates concurrent splices on three replicas, with late, reordered and repeated deliveries. It also compares one replica with a `String`, and it applies the edits of `edits_since` to the text of the old version. Two concurrent `Editor::set_text` edits of different parts of a text both remain.
- `tests/reference.rs` prints and parses generated references.
- `Editor::splice` and `Document::merge` assert in every build that each edit has a writer. `Editor::splice` also asserts that the text changed only in the range of the splice.

## The stored format

`crates/nit-document/tests/snapshots` contains one snapshot for each stored format, with its text in a Markdown file. Changes that cannot load an old snapshot fail `stored_snapshots_load`. Correct `Document::load` then, and never the files. To add a format, run the ignored test `write_snapshot` with a new file number.

## The author's side

`nit doc show` prints the text exactly as the server stores it, and `nit doc edit` replaces one exact text. `nit doc --help` lists the commands. The plugin skill `nit:document` instructs agents how to use them.

Reviewers send documents to authors with a button. `nit watch` then posts the lines that the reviewers changed to the session of the author, in the form that `nit doc show --since` prints. The watch stores the sent version of each document in its cursor file, and the next diff starts at that version. For documents with no stored version, the diff starts at `Document::last_edit_by` of the author, so it contains only the edits that reviewers made after the last edit of the author.

## Generated files

`nix run .#gen-types` writes `web/src/api/document.gen.ts` from the `ts` feature of `nit-document`. The `types-drift` check fails when that file is stale.
