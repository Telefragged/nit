---
name: simplified-english
description: >
  One idea per sentence, 20 words for an instruction and 25 for a
  description, six sentences per paragraph, active voice. Load before
  writing any user-facing prose: chat, commits, comments, nit replies,
  docs, UI copy. From ASD-STE100.
---

# Simplified English

Everything this repo emits to a human obeys these rules: chat output,
commit messages, code comments and doc-comments, nit comments and
replies, `docs/` pages, plan documents, and UI strings.

Two neighbours own the adjacent questions. `comment-style` decides which
home a fact belongs in and whether to write it at all, and
`CLAUDE.local.md` decides which facts to surface. This skill decides how
the surviving sentence is built, so apply it last.

Quoted output, log lines, error text and identifiers are reproduced
exactly. Never rewrite them to fit a cap.

<caps>

Limits, not targets. A sentence under the cap that carries two ideas
still fails.

| Unit                                          | Limit       | STE |
| --------------------------------------------- | ----------- | --- |
| A sentence that tells someone to do something | 20 words    | 5.1 |
| A sentence that describes something           | 25 words    | 6.3 |
| Any sentence                                  | 1 idea      | 4.1 |
| An instruction                                | 1 action\*  | 5.2 |
| A paragraph                                   | 1 topic     | 6.5 |
| A paragraph                                   | 6 sentences | 6.6 |
| A multi-word noun                             | 3 words     | 2.1 |

\* Unless the actions happen at once: "Remove and discard the seal."

Counting (STE 8.4 thru 8.7). Each of these is **one word**: a number or
a number with its unit; an abbreviation, alphanumeric identifier or
proper noun; any text inside parentheses, however long; a hyphenated
word; and here also any code span or path. A colon opening a vertical
list ends the sentence, and each item counts on its own.

</caps>

<budget>

The caps shape a sentence. They do not stop a reply from running long,
and a reply built from short sentences can still ramble. So the whole
answer has a budget too, set by the channel.

| Channel              | Budget                                |
| -------------------- | ------------------------------------- |
| Chat reply           | 2 paragraphs, 2 to 3 sentences each   |
| nit comment or reply | The decision, then at most one reason |
| Commit subject       | 60 characters                         |
| Commit body          | The why, wrapped at 72 columns        |
| Code comment         | The non-obvious why, and nothing else |
| `docs/` page         | What the reader needs to act          |

A request for detail resets the budget. When someone asks you to explain
something fully, the full explanation is the answer, and withholding it
is not concision.

**Answer first, then stop.** The answer goes in the first sentence, the
reason after it. Everything past the reason is there because you wanted
to write it. Agreement is not an answer, so a reply never opens by
conceding the point or restating the question.

_You are right on both counts. I looked at this and the fold reads an
owned snapshot, so I have narrowed the lock._ → _Narrowed the lock to
the insert. The fold reads an owned snapshot._

Three habits break the budget, and each is easy to mistake for
thoroughness:

- **Sections nobody asked for.** "What not to change", "How to confirm
  this", "Some background" — each is a question you invented and then
  answered.
- **The closing restatement.** A last paragraph saying the reply again
  in shorter words. The reader just read it.
- **The alternative you rejected.** If it is not what you recommend, it
  does not need a paragraph.

Headings in a chat reply are a warning sign. Two paragraphs need none,
so reaching for one means the reply outgrew its budget.

Length is the reader's cost, not a measure of your effort.

</budget>

<sentences>

**One idea per sentence** (STE 4.1, 6.1) does most of the work. A
sentence carries one subject, the next carries the next one. Two
subjects welded with "and", "which", or a trailing participle is the
commonest failure, so split at the weld.

<example type="two ideas in one sentence">

Do not write:

> The sweep now reads the change snapshot per member rather than folding
> the chain, which makes the graph endpoint cheap, and it also fixes the
> stale-count bug that showed up when a revision landed mid-sweep.

Write:

> The sweep reads the change snapshot per member. It no longer folds the
> chain, so the graph endpoint is cheap. This also fixes the stale count
> when a revision lands mid-sweep.

</example>

The rest, each with its failure and its fix:

**Active voice** (3.6). Ask "by whom?" — if the sentence answers,
rewrite with that actor as the subject. Passive is permitted only when
the actor is genuinely unknown.
_The chain is rebased by the merge script_ → _The merge script rebases
the chain._

**A verb, not a nominalization** (3.7). "Performs a validation of",
"gives an indication of", "the removal of" pad the sentence and hide the
action.
_Before the removal of the plan commit_ → _Before you remove the plan
commit._

**Simple tenses only** (3.2, 3.4). Present, past, future, imperative. No
perfect tenses, no stacked auxiliaries.
_The endpoint has been updated so that it will be returning the new
shape_ → _The endpoint returns the new shape._

**Every sentence in full** (4.2, 4.5). No dropped subjects, verbs,
articles or contractions. Deleting words makes a sentence harder to
read; shorten by splitting instead.
_Can't reproduce — probably a race_ → _I cannot reproduce this. It is
probably a race._

**Keep "that"** (GR-1). It marks where the main clause ends.
_Make sure the worktree is clean_ → _Make sure that the worktree is
clean._

**No phrasal verbs** (9.3). A verb plus a preposition takes a meaning
neither part has, and the abstract reading is rarely the one you want.
_The sweep picks up the revision and the cache gets blown away_ → _The
sweep reads the revision and clears the cache._

**No Latin abbreviations** (GR-6). Write "for example", "that is", "and
so on", or drop the aside.

**Rewrite, do not word-swap** (9.1). When a sentence resists these
rules, the sentence is wrong, not the words.

**Watch "with"** (GR-2). "Install the panel with the green fasteners"
has three readings. Say which.

**Gender-neutral throughout** (GR-7). Use "they" for a person whose
pronouns you have not been told.

**The possessive only when plainly correct** (GR-8). Otherwise use "of".

</sentences>

<structure>

**Open each paragraph with its topic sentence** (6.4, 6.5). The first
sentence names the topic and the rest explain it. Read only the topic
sentences and you should get the outline.

**One topic per paragraph, six sentences at most** (6.5, 6.6). Past six,
split. A second topic is a second paragraph however short the first is.

**Connect related sentences** (4.4, 6.2) with "and", "but", "then",
"thus", "as a result", "at the same time". They tell the reader whether
what follows is new, contrary, or consequent.

**Put a series in a vertical list** (4.3). A sentence that must carry
many items becomes a colon and a list. End an item with a period only
when it is a full sentence, never with a comma or semicolon, and always
put a period after the last item.

**Repeat the wording for a repeated action** (9.4, 1.11). One thing gets
one name and one recurring step gets one phrasing, every time.
Variation for its own sake reads as a difference in meaning. The
vocabulary is fixed by `crates/nit-types/src/domain.rs`.

</structure>

<instructions_and_punctuation>

**Instructions are imperative** (5.3). "Run `nix flake check`", not "the
check should be run".

**Condition first, then the command, split by a comma** (5.4). "If the
rebase conflicts, resolve it and re-run treefmt."

**A note informs, never instructs** (5.5). If it tells the reader to do
something, make it a step.

**A warning names the risk** (7.1 thru 7.3): the level of risk, the
command or condition, then what happens if it is ignored.

**No semicolons** (8.1) — use a period. **Hyphenate** a compound
modifier before a noun and a shape-plus-noun term (8.2): `line-level
comment`, `O-ring`. **Parentheses** hold a reference, identifier,
abbreviation or short alternative (8.3), never an aside carrying an
idea. That is its own sentence.

</instructions_and_punctuation>

<failure_modes>

The rules above, stated as the symptom to catch in your own draft.

| Symptom                                                 | Rule   |
| ------------------------------------------------------- | ------ |
| A sentence you re-read to find the subject              | 4.1    |
| "and", "which" or "while" joining two independent facts | 4.1    |
| Hedging — "it's worth noting", "arguably", "somewhat"   | 4.1    |
| "In order to", "the fact that", "at this point in time" | 3.7    |
| "was updated", "is handled", "gets called"              | 3.6    |
| A parenthesis carrying a whole second thought           | 8.3    |
| A seventh sentence in a paragraph                       | 6.6    |
| The same step phrased two ways in one document          | 9.4    |
| An answer arriving after the reasoning                  | 6.4    |
| A section answering a question nobody asked             | budget |
| Headings in a two-paragraph chat reply                  | budget |
| A closing paragraph that says the reply again           | budget |

Check a draft in this order: cut what has no home (`comment-style`),
split what carries two ideas, then count.

</failure_modes>

<provenance>

From ASD-STE100 Issue 9, Part 1. Rules 1.1 thru 1.6, 1.12, 3.1, 9.2,
GR-3 and GR-5 are omitted, because each depends on the controlled
dictionary in Part 2. Rules 1.7 thru 1.11, 1.13 and 1.14 concern
technical nouns and fold into `<structure>` and the domain model.

Where STE and this repo disagree, this repo wins. American spelling
(1.14) holds. The 72-column wrap on commit messages is a separate rule,
unaffected by the word caps here.

</provenance>
