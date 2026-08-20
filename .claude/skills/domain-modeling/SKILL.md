---
name: domain-modeling
description: >
  How a domain type is built and reviewed in nit. Load this BEFORE
  adding or changing any type in `crates/nit-types`, and at the start of
  every review pass.
---

# A domain type cannot be wrong

<the_bargain>

Check an invariant once, at construction, and every function downstream
inherits it: no error to return, no invalid case to branch on, no test
for a state that cannot exist. Check it late and every caller carries
the doubt forever.

Two moves buy that, and the rest of this skill is how they land:

1. **Validate on the way in.** The constructor is the only door, and the
   only place that says no.
2. **Make illegal states unrepresentable.** A state the type cannot
   spell is a state nothing has to check, handle, or test.

The measurable form: a `Result` in `crates/nit-types/src/domain/` that
is not a constructor is an invariant being checked too late. That holds
today — every one of them sits on `new`, `try_from`, or `from_str` —
so it is a rule, not an aspiration.

</the_bargain>

<the_two_tiers>

Sort every type by one question: **can construction be wrong?**

|               | Yes — a validated value          | No — a snapshot or a plain wrapper                   |
| ------------- | -------------------------------- | ---------------------------------------------------- |
| Examples      | `Sha`, `ChangeId`                | `RevisionNumber`, `ChangeStatus`, `ChangeProjection` |
| Constructor   | `new(..) -> Result<Self, Error>` | `new(..) -> Self`, or none                           |
| Payload       | private, behind accessors        | private on a newtype; `pub` fields on a projection   |
| `Deserialize` | only through `try_from`          | derived                                              |

A projection is what a fold produced. It has no invariant of its own, so
its fields stay `pub` and it reads as the data it is — accessors around
it buy nothing. A newtype is different even when it cannot fail:
`RevisionNumber(pub u64)` invites arithmetic on a number that means
something, so the payload is private and the operations that make sense
(`previous`, `get`) are the ones offered.

</the_two_tiers>

<construction>

A validated type carries its rules on `new`, and nowhere else.

<example type="validation as an effect">

```rust
// BAD — the vocabulary lives in a free function, the type is built
// beside it, and a caller that forgets the call still compiles.
pub struct Sha(pub String);

pub fn validate_sha(s: &str) -> Result<(), String> { ... }
```

```rust
// GOOD — one door. There is no way to hold a Sha that skipped the check.
pub struct Sha(String);

impl Sha {
    /// A git object name: 40 hex characters.
    ///
    /// # Errors
    ///
    /// [`ShaError`] names which rule the input broke.
    pub fn new(s: impl Into<String>) -> Result<Sha, ShaError> { ... }

    #[must_use]
    pub fn as_str(&self) -> &str { &self.0 }
}
```

</example>

- **The error is a type**, derived with `thiserror`, naming which
  invariant broke — not a `String`. `Display` renders it, so the API
  envelope is unchanged and a caller that wants to branch still can. A
  closed-set `FromStr` is the exception: its only failure is a value
  outside the set, and it has nothing to name.
- **No infallible conversion into a validated type.** A `From<&str>`
  next to a validating `new` is a hole in the gate, however convenient
  it is in tests.
- **Never construct just to check.** `Sha::new(s).is_ok()` throws away
  the value the parse just produced; keep it, or take the branch that
  needs it.

</construction>

<serde_is_the_gate>

A derived `Deserialize` builds a type field by field, straight past the
constructor. On a validated type that makes the invariant a suggestion —
and it applies to every source, including log payloads read back out of
sqlite.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Sha(String);

impl TryFrom<String> for Sha {
    type Error = ShaError;
    fn try_from(s: String) -> Result<Sha, ShaError> { Sha::new(s) }
}
```

`TryFrom<String>` takes the string serde already owns. `FromStr` is for
types that genuinely parse a `&str` into something else — a number, an
enum — not for one that keeps the string it was given.

Two mechanics you will hit immediately:

- **`#[serde(transparent)]` and `try_from` cannot combine.** Serde
  refuses the pair, so a gated newtype writes its own `Serialize` — one
  line of `serialize_str`. The wire shape is unchanged.
- **A type with several fields needs somewhere to land first.** Give
  `try_from` a private struct in the same module, named for what the
  parts are before they are the type (`Selection` → `CommentRange`).

**A new invariant must hold for data already written.** Deserialization
gates the database too, so a stored value that fails the new rule makes
its change unfoldable. The type gives no hint that this is at stake. A
row read back parses like anything else, and `db.rs` turns a failure
into a `FromSqlConversionFailure` rather than a panic.

**Reshaping a stored type is the deserializer's job, not a migration's.**
The log is append-only, so an old entry keeps the spelling it was written
in. Let the type read both — a private struct holding either shape, and a
`From` that resolves it — and nothing downstream learns that a second
spelling exists. Rewriting stored JSON in SQL costs more and buys less.

</serde_is_the_gate>

<illegal_states>

- **A closed set of values is an `enum`, never a `String`.** Sides,
  verdicts, statuses, kinds. The serde `rename`/`rename_all` fixes the
  wire spelling, so the same type is the domain value, the JSON shape,
  and the parsed CLI input: an exhaustive `match` with no `_ =>` arm, no
  `as_str`/`from_str` round-trip at the boundary, and an unknown value
  that fails deserialization cleanly instead of flowing deeper as a
  string.
- **A cluster of `Option`s is an enum.** If some combinations are
  nonsense, the type is wrong.

<example type="options that encode a state">

```rust
// BAD — 32 spellings, 3 of them legal. Every reader re-derives which.
pub struct ThreadLocation {
    pub file: Option<String>,
    pub line: Option<u64>,
    pub side: Option<Side>,
    pub range: Option<CommentRange>,
    pub line_text: Option<String>,
}
```

```rust
// GOOD — the three legal shapes, and nothing else.
pub enum Anchor {
    Change,
    File { file: String },
    Line { file: String, side: Side, line: u64, ... },
}
```

</example>

A normalizing function that turns the loose shape into the tight one
right after parsing is the tell: the tight shape should have been the
type all along.

</illegal_states>

<the_wire>

Domain types are the HTTP/JSON contract, so a wire shape and a domain
shape that disagree have to be reconciled somewhere.

**Move the wire first.** Serde is expressive enough to serialize most
well-modeled types directly — enum tagging in particular. A wire format
chosen to suit a weaker type is a reason to change the format, not the
type.

**A DTO only when they cannot reconcile.** It lives in the module behind
its route, carries a `Dto` suffix so a call site importing both cannot
confuse them, and is what ts-rs exports. The domain type is then
Rust-only, and the conversion happens once, at the route.

</the_wire>

<naming>

**Terms are spelled out.** An identifier carries a domain term in full,
so the term's definition is the only thing a reader needs to understand
the name. The exemptions are a closed set — `id`, `sha`, `repo`,
`git_dir` — and adding to it is a decision, not a convenience.

A term is defined once, on the type that carries it, in
`crates/nit-types/src/domain.rs`; a shape that exists to serve a route
belongs to that route's module. `comment-style` covers how those
definitions are written.

</naming>

<tests>

A well-modeled type deletes its own tests. Nothing needs to assert that
an illegal state is rejected once it cannot be spelled.

What is left is the constructor's vocabulary, and it belongs in a
doctest on `new`: one accepted value, one rejected. That is the example
a reader needs and the test at the same time. A unit-test module
enumerating rejections restates the constructor; a unit test asserting
an impossible state is refused restates the compiler.

</tests>

<reviewing>

Run these before reading. They find the mechanical violations, not all
of them — a type whose invariant is checked three functions later
passes every one of them.

```sh
# Every hit must sit on new / try_from / from_str.
rg -n 'Result<' crates/nit-types/src/domain/

# A validated type with a public payload, or a public field.
rg -n 'pub struct \w+\(pub|^\s+pub \w+:' crates/nit-types/src/domain/

# A validated type must appear here; an infallible one must not exist.
rg -n 'serde\(try_from' crates/nit-types/src/domain/
rg -n 'impl From<(&str|String)> for' crates/nit-types/src/domain/
```

Then ask, for each `Result` the type hands back: **who calls this, and
what do they do with the error?** If no caller can act on it, the check
belonged at construction and the function should not be fallible.

</reviewing>
