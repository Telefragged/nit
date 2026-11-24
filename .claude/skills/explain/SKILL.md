---
name: explain
description: >
  Write a nit document that shows reviewers the public surface of a
  scope: "the changes", "change 412", a crate.
argument-hint: <scope>
disable-model-invocation: true
---

# Explain a scope for review

Scope: $ARGUMENTS

Write one nit document that shows reviewers the public surface of that scope. The scope is what the user gave: the changes of this branch, one change, a crate, a feature. The reviewers of this repo judge the public surface first, and the code behind it only where the surface gives a reason. The plugin skill `nit:document` documents the commands and the form of references.

## What to reference

Use this order. Omit a part that the scope does not contain.

1. **The HTTP API.** One reference for the list of routes, and one for each request and response type.
2. **The Rust API of a library crate.** One reference for each public item that a consumer of the crate uses: a type, a trait, a function. Give the lines of the item only, from its doc-comment to its last line. For a type with methods, give the block of its public methods as a second reference. Do not reference a whole file, a private helper, or a public item of an inner module that the crate does not export.
3. **The CLI.** Attach the `--help` output of each command in the scope, and reference the attachment. Attach an example of the output when its format is new. Reference no CLI code.
4. **The UI.** Select the pages and components that matter for the review: one image for each state that the scope changes for users. Attach a screenshot or an HTML page of each, and reference the attachment.
5. **The properties and invariants.** State each rule that the intention of the user gives the code, and reference the place that enforces it: a type that makes a wrong state unrepresentable, an assert, a property test. Reference a test only when it is the statement of such a rule.

Use the latest revision of each change. Attach a file to the change that makes the surface.

Write one or two sentences before each reference: what the item is for, and what reviewers must judge.

## A crate that references can show

Each reference shows the lines of one item, so a library crate has this layout:

- `lib.rs` is the list of the public API. Each module is private, and `lib.rs` exports each public item by its identifier. A public module contains only items that are all part of the API, such as the shapes of a wire format.
- The first `impl` block of a type contains its public methods, and nothing else. A second `impl` block contains the private and the test-only methods.
- A public item has a doc-comment.

`crates/nit-document` has this layout.
