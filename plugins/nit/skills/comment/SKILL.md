---
name: comment
description: Communicate with the reviewer through nit instead of the chat — open and reply to threads with `nit comment`, anchored as tightly as possible (range > line > change-level), resolving settled notes and leaving open the ones the reviewer should weigh in on. Use to record a decision, answer a review thread, or raise a question for the reviewer.
---

# nit:comment — talk to the reviewer in nit

nit is the single source of truth for the review conversation. Notes,
decisions, questions, and answers belong here — anchored to the code they're
about — not scattered through the chat.

## Open or reply to a thread

```sh
# open a NEW thread, anchored to code:
nit comment --change-id <Change-Id> --file <path> --line <n> [--range S-E] [--side new|old] -m "…"
# reply to an EXISTING thread (anchor flags ignored):
nit comment --change-id <Change-Id> --thread <thread-id> [--resolve|--unresolve] -m "…"
```

Target a change by the `Change-Id:` trailer on the commit you're commenting on,
with `--change-id <Change-Id>`. (`--change <id>` takes the numeric change id
instead, for when a human hands you one.) A range is `START-END`, each endpoint
`line:char` (e.g. `42:8-42:30`).

Write the body as markdown (GFM + hard line breaks). Quote code in a fenced
block with a language tag — it renders with the same syntax highlighting as
the diff, so the quote reads like a reference:

````sh
nit comment --change-id <Change-Id> --file src/queue.rs --line 42 -m 'Bounded:

```rust
let (tx, rx) = mpsc::channel(64);
```'
````

## Anchor as tightly as you can

Pin the note to the smallest span that carries it. **Prefer a range, then a
line, then a change-level comment** — the tighter the anchor, the clearer what
it is about.

```sh
# RANGE — best: the exact characters
nit comment --change-id <Change-Id> --file src/queue.rs --line 42 --range 42:8-42:30 \
  -m "Bounded channel over unbounded — backpressure matters more than never
      blocking the producer."
# LINE — when the whole line is the point
nit comment --change-id <Change-Id> --file src/config.rs --line 14 -m "Defaulted to 30s."
# CHANGE-LEVEL — last resort, when it isn't about specific lines
nit comment --change-id <Change-Id> -m "Skipped the legacy migration — assuming no live data."
```

Leave a thread **open** when the reviewer should weigh in; add `--resolve` for
a settled note that needs no reply. When you answer review feedback, reply on
the thread it concerns and `--resolve` it once it's handled.

## Keep the conversation in nit, not the chat

Almost everything you'd say belongs in nit, not the conversation — decisions,
rationale, questions, and general remarks go in as comments. Don't narrate your
progress in the chat. The only thing you send to the conversation is a
**brief** note when you hand off and wait for review — a line or two, no more.
Your terminal is the channel only when the user prompts you there directly.
