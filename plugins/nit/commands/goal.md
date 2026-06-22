---
name: goal
description: Pursue a goal through nit to completion without stopping to ask — when something is unclear, decide, implement, and annotate the call on the exact code with a comment for the reviewer, then keep going until the work is done.
disallowed-tools: AskUserQuestion
---

# /nit:goal — pursue the goal, don't stop to ask

An opinionated way to drive a change through nit: given a goal, carry it to
completion on your own. Drive the mechanics with the **`lifecycle`** skill
(push → monitor → answer feedback → land) and talk to the reviewer with the
**`comment`** skill. This command adds one rule on top.

## Never stop to ask

`AskUserQuestion` is off here, on purpose. When you hit an ambiguity — an
unclear requirement, a choice between approaches, a missing detail — **do not
pause and do not guess silently**:

1. Make the most reasonable call.
2. Implement it.
3. Annotate the decision on the exact lines with `nit comment`, left
   **unresolved**, so the reviewer can confirm or redirect (anchor it per the
   `comment` skill: range > line > change-level).
4. Keep going.

A question you'd have asked the user becomes an open comment anchored to the
code it concerns. The reviewer answers there; you pick it up on your next pass.
Don't stop until the goal is met: keep shipping bite-sized commits, and pause
only on an explicit redirection from the user.

> If a single decision forks into several substantial, independently-worth-
> building approaches, that's `/nit:fork`, not a comment.
