---
name: comment-style
description: >
  Where a fact belongs when working on nit: the domain model carries
  what a term is, code comments the non-obvious why, doc-comments the
  contract, commit messages the why of the change, nit threads the
  review history — each fact in exactly one home. Load this BEFORE
  writing or amending any code, comment,
  doc-comment, or commit message in this repo, and at the start of every
  review pass — not after a draft exists. It also sets the tone the code
  itself is written in: reasonably concise. A /simplify or /code-review
  run here must also spawn the comment-audit agent this skill defines.
---

# One home per fact

<one_home_per_fact>

Every fact has exactly one home. Written twice it drifts — one copy gets
updated, the other keeps asserting something false. Written in the wrong
home it ages into noise: review chatter fossilized in a source file,
code narration padding a commit message.

| Home           | Carries                                        | Reader                                     |
| -------------- | ---------------------------------------------- | ------------------------------------------ |
| Domain model   | what a term _is_                               | anyone meeting the term for the first time |
| Code comment   | the non-obvious _why_ of the code as it is     | someone asking "why is the code this way"  |
| Doc-comment    | the contract: what a caller may rely on        | someone who has NOT read the body          |
| Commit message | the _why_ of this change                       | someone asking "why was this introduced"   |
| nit thread     | the review history: questions, verdicts, fixes | reviewer and author, during review         |

The domain model is `crates/nit-types/src/domain.rs`: a term is defined
once, on the type that carries it, and that type lives there, so the
module reads as the whole vocabulary. A doc-comment elsewhere may rely
on that meaning instead of restating it. A definition holds only what a
code change cannot falsify — never a count, a route, a column, a status
code or a behavior, all of which belong to the code they describe.

The failure mode is leakage downward: review history into commit
messages, change rationale into code comments, code narration
everywhere. What a fact _is_ picks its home, not where you happen to be
typing.

</one_home_per_fact>

<code_comments>

Code reads as if this were its first, best version. Test every comment
you write or review:

1. **Narration — delete it and reread.** If the code alone tells the
   reader the same thing, the comment is narration: it costs on every
   read, and when the code changes and the comment does not, the file
   lies.
2. **History — does it read cleanly to someone who has only ever seen
   this version?** References to an earlier version, an alternative not
   taken, or a review exchange ("no longer", "instead of", a defensive
   justification of a questioned choice) are history. Delete them. Brags
   ("no JSON, so it never serializes!") count — they answer a critique
   the reader never saw.
3. **Keeper — does it state something the code cannot?** An invariant, a
   constraint, a subtle ordering, a reason the obvious alternative is
   wrong _in the world_, not in the change's history. Only these earn
   their place. Be dense: the fewest words that carry the rationale.

<examples>

<example type="narration — delete">

```rust
// Collect the approved changes from the chain.
let approved: Vec<_> = chain.changes.iter().filter(|c| c.approved).collect();
```

</example>

<example type="history — delete">

```rust
// Uses the pooled connection now instead of opening one per request.
let conn = pool.get().await?;
```

"now / instead of" compares against a version the reader cannot see.

</example>

<example type="brag — delete">

```rust
// Operates entirely on typed data — no JSON, so it never serializes!
```

</example>

<example type="keeper — invariant the code cannot state">

```rust
// The write lock makes this id allocation race-free against a
// concurrent push.
let id = state.next_id;
```

The locking discipline is invisible in these lines; without the comment
a reader reconstructs it from the whole call graph.

</example>

</examples>

</code_comments>

<doc_comments>

A doc-comment's reader has **not** read the body — they see it in
rustdoc, an IDE hover, a generated types file. So restating the _what_
is welcome: `/// Pushes an element to the back of the vector.` is a fine
doc for `Vec::push`.

The first paragraph is also the item's **summary** — rustdoc lifts it
into module item tables and search results, and IDE hovers lead with it.
Keep it to one line in the third person present indicative (`Returns the
tip`, not `Return the tip`); those tables are fixed-width, so past about
15 words it wraps and the list stops being skimmable. Everything after
that first sentence goes below the blank line, where it costs a skimmer
nothing.

<example type="summary line — split the detail out">

```rust
/// Returns the change the reviewer is looking at, resolving to the
/// chain tip when the request names no revision and to the latest
/// approved change otherwise.
```

Three lines of item table for one entry. Split at the first sentence:

```rust
/// Returns the change the reviewer is looking at.
///
/// Resolves to the chain tip when the request names no revision, and
/// to the latest approved change otherwise.
```

</example>

clap's derive reads that shape differently: on an `#[arg]` or
`#[command]` field the first paragraph is the `-h` text and the whole
comment is `--help`. Splitting one moves its qualifiers out of `-h` — a
CLI change, not a docs change. Leave those as one paragraph.

What a doc-comment must carry is the **contract**: invariants the caller
may rely on, panics, error conditions, ordering guarantees.
`crates/nit-types` doc-comments _are_ the wire contract — their
semantics bind both the server and the generated TypeScript.

In Rust, show the contract as an example under an `# Examples` heading
(the rustdoc convention): a doctest documents and verifies in one
artifact, so it cannot drift. Where it covers what a unit test would
assert, write it _instead of_ that test — it replaces the tests it
covers, never the test module wholesale. Keep unit tests for what would
bloat docs: edge-case matrices, failure paths, concurrency.

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

TypeScript has no doctest runner: a TSDoc example never executes and
rots silently. For exported TS API, state the contract in prose and skip
examples unless they are short enough to be obviously true.

</doc_comments>

<commit_messages>

The subject states the _what_ (component-prefixed); the body states the
_why_ — the reasoning someone needs when they arrive at this commit
asking why the change was introduced at all. 72-column wrap, per
CLAUDE.md rule 2.

<example type="subject = what, body = why">

```
web: update pending badge color to yellow

The blue color could easily be mistaken for the commented badge on a
quick glance. No other badge uses yellow, so it reads unambiguously.
```

</example>

Never invent a rationale. If the why is not obvious, do not guess at one
— ask the reviewer for the reasoning (the `nit:comment` skill) and let
the answer land in the message on the next amend.

The message carries no review history: what round three asked for and
how revision four answered it has no place in it. `a = a + 1` becoming
`a += 1` after review needs no trace — the message describes the change
as if it had always been written that way.

</commit_messages>

<comment_audit>

Every review pass applies the three tests in `<code_comments>`. A
`/simplify` or `/code-review` run here additionally spawns **one extra
read-only agent** for this audit: point it at this file and the diff
under review; it proposes, the orchestrator applies.

The burden of proof is on removal — presume nothing, prove each flag:

- **Narration**: quote the adjacent code stating the same fact.
- **History / brag**: name what it references outside this version — the
  earlier code (git blame), the review exchange (nit thread) — or show
  the sentence is incoherent in a first-version reading.
- A doc-comment restating the _what_ is **not** a finding; that is its
  job.
- Unproven flags are dropped, the comment stays. Length or phrasing
  taste is never grounds.

Report findings in this shape, one per flagged comment:

<finding>
file: crates/nit/src/chain.rs:42
comment: "// Collect the approved changes from the chain."
verdict: narration
proof: line 43 reads `.filter(|c| c.approved).collect()` — the same fact.
action: delete
</finding>

`action` is one of `delete`, `move to commit message`, or `answer in nit
thread` — pick between them with `<one_home_per_fact>`.

</comment_audit>

<tone_preference>

Write the code itself reasonably concise. A reader's budget is
attention, and every line, binding, parameter and layer of indirection
spends some of it whether or not it earns it.

- **Prefer the shorter form when it reads at least as clearly.** A
  binding used once, a wrapper that only forwards, a `match` with one
  real arm — each is a hop the reader takes for nothing.
- **Do not pad for cases that cannot happen**: an option no caller
  passes, a trait with one implementor, a fallback for an error the
  types already rule out. Remove before you rewrite, rewrite before you
  add (CLAUDE.md rule 8).
- **Say it once.** Two sites computing the same fact drift apart the
  moment one is updated — hoist it, or give it one owner.
- **Concise is not terse.** Compression the reader has to undo — a
  clever one-liner, a chain five combinators deep, single-letter names
  in a wide scope — costs more than the lines it saves. Name things for
  the distance they travel: short in a tight scope, spelled out where
  the scope is wide.

</tone_preference>
