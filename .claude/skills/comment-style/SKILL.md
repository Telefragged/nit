---
name: comment-style
description: >
  Where a fact belongs when working on nit: code comments carry the
  non-obvious why, doc-comments the contract, commit messages the why of
  the change, nit threads the review history — each fact in exactly one
  home. Load this BEFORE writing or amending any code, comment,
  doc-comment, or commit message in this repo, and at the start of every
  review pass — not after a draft exists. A /simplify or /code-review run
  here must also spawn the comment-audit agent this skill defines.
---

# One home per fact

Every fact has exactly one home. A fact written twice drifts: one copy
gets updated, the other keeps asserting something false with full
confidence. A fact written in the wrong home ages into noise — review
chatter fossilized in a source file, code narration padding a commit
message. The four homes:

| Home           | Carries                                        | Reader                                    |
| -------------- | ---------------------------------------------- | ----------------------------------------- |
| Code comment   | the non-obvious _why_ of the code as it is     | someone already reading this code         |
| Doc-comment    | the contract: what a caller may rely on        | someone who has NOT read the body         |
| Commit message | the _why_ of this change                       | someone asking "why is the code this way" |
| nit thread     | the review history: questions, verdicts, fixes | reviewer and author, during review        |

The failure mode is leakage downward: review history leaking into commit
messages, change rationale leaking into code comments, code narration
leaking everywhere. Each section below is one home's rules.

<instructions>

## Code comments

Code reads as if this were its first, best version. Apply three tests to
every comment you are about to write (or are reviewing):

1. **Narration test — delete it and reread.** If the code alone tells the
   reader the same thing, the comment is narration. Narration costs on
   every read and drifts: the code changes, the comment above it does
   not, and now the file lies.
2. **History test — does it read cleanly to someone who has only ever
   seen this version?** A comment that references an earlier version, an
   alternative not taken, or a review exchange — "no longer", "instead
   of", "we don't X here because…", or a defensive justification of a
   choice someone once questioned — is history. The why of the change
   belongs in the commit message; the review exchange already lives in
   its nit thread. Brags ("no JSON, so it never serializes!") are
   history too: they answer a critique the reader never saw.
3. **Keeper test — does it state something the code cannot?** An
   invariant, a constraint, a subtle ordering, a reason the obvious
   alternative is wrong _in the world_, not in the change's history.
   These comments are the only ones that earn their place. Be dense: the
   fewest words that carry the rationale.

<examples>

<example type="narration — delete">

```rust
// Collect the approved changes from the chain.
let approved: Vec<_> = chain.changes.iter().filter(|c| c.approved).collect();
```

The code states this verbatim; the comment is a second copy that can
drift.

</example>

<example type="history — move the why to the commit message">

```rust
// Uses the pooled connection now instead of opening one per request.
let conn = pool.get().await?;
```

"now / instead of" compares against a version the reader cannot see.
Git blame holds the old version; the commit message holds why it
changed.

</example>

<example type="brag — delete">

```rust
// Operates entirely on typed data — no JSON, so it never serializes!
```

A defense against a critique from a review round. The reader never saw
the critique; the thread that raised it already records the answer.

</example>

<example type="keeper — invariant the code cannot state">

```rust
// The write lock makes this id allocation race-free against a
// concurrent push.
let id = state.next_id;
```

Nothing in these lines shows the locking discipline; without the
comment a reader must reconstruct it from the whole call graph.

</example>

</examples>

## Doc-comments

A doc-comment's reader has **not** read the body — they see it in
rustdoc, an IDE hover, a generated types file. So the rules differ from
code comments in one way: restating the _what_ is welcome. `/// Pushes
an element to the back of the vector.` is a fine doc for `Vec::push`,
where the same sentence inside the body would be narration.

What a doc-comment must carry is the **contract**: invariants the
caller may rely on, panics, error conditions, ordering guarantees.
`crates/nit-types` doc-comments are the wire contract itself — their
semantics bind both the server and the generated TypeScript.

In Rust, prefer showing the contract as an **example** under an
`# Examples` heading (the rustdoc convention): a doctest documents and
verifies in one artifact, so it cannot drift. Where a doctest naturally
covers what a unit test would assert, write the doctest _instead of_
the unit test — it replaces the tests it covers, never the test module
wholesale. Keep unit tests for what would bloat docs: edge-case
matrices, failure paths, concurrency.

<example type="doctest carrying the contract">

````rust
/// Splits a trailer line into key and value.
///
/// Returns `None` when the line carries no `:` — a plain body line,
/// not a malformed trailer.
///
/// # Examples
///
/// ```rust
/// assert_eq!(split_trailer("Change-Id: I123"), Some(("Change-Id", "I123")));
/// assert_eq!(split_trailer("just prose"), None);
/// ```
pub fn split_trailer(line: &str) -> Option<(&str, &str)> { … }
````

</example>

TypeScript has no doctest runner: an example in TSDoc never executes
and rots silently. For exported TS API, state the contract in prose and
skip examples unless they are short enough to be obviously true.

## Commit messages

The subject states the _what_ (component-prefixed); the body states the
_why_ — the reasoning a future reader needs when they ask "why is the
code like this" and the code itself cannot answer. 72-column wrap, per
CLAUDE.md rule 2.

<example type="subject = what, body = why">

```
web: update pending badge color to yellow

The blue color could easily be mistaken for the commented badge on a
quick glance. No other badge uses yellow, so it reads unambiguously.
```

</example>

Never invent a rationale. If you were asked to make a change and the
why is not obvious, do not guess at one for the body — open a thread on
the commit message itself (the `nit:comment` skill) asking the reviewer
for the reasoning, and let the answer land in the message on the next
amend.

The commit message also never carries review history: what round three
asked for and how revision four answered it lives in the nit threads,
nowhere else. `a = a + 1` becoming `a += 1` after review needs no
trace in the message — the message describes the change as if it had
always been written that way.

## The comment audit (review passes)

Every review pass applies the three tests above. A `/simplify` or
`/code-review` run on this repo additionally spawns **one extra
read-only agent** dedicated to this audit: point it at this file and
the diff under review; it proposes, the orchestrator applies.

The audit's burden of proof is on removal — presume nothing, prove
each flag:

- **Narration**: quote the adjacent code stating the same fact.
- **History / brag**: name what it references outside this version —
  the earlier code (git blame), the review exchange (nit thread), or
  show the sentence is incoherent in a first-version reading.
- A doc-comment restating the _what_ is **not** a finding — that is its
  job.
- Unproven flags are dropped: the comment stays. Length or phrasing
  taste is never grounds.

Report findings in this shape, one per flagged comment:

<finding>
file: crates/nit/src/chain.rs:42
comment: "// Collect the approved changes from the chain."
verdict: narration
proof: line 43 reads `.filter(|c| c.approved).collect()` — the same fact.
action: delete
</finding>

`action` is one of `delete`, `move to commit message` (the fact is the
why of this change), or `answer in nit thread` (the fact is review
history).

</instructions>
