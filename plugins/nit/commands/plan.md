---
name: plan
description: Plan mode for nit. Use it for the same work that warrants Claude Code's plan mode — a new feature, a refactor, several viable approaches, a multi-file or architectural change, or requirements you can only pin down by exploring first. You research read-only and form the plan as plan mode would, but instead of presenting it to the user you push it as a reviewable commit (plan/<name>.md) and raise every question you'd have asked as an inline comment on it. Implement once nit approves it. Skip it for a one-line or obvious fix and for pure research.
disallowed-tools: AskUserQuestion, EnterPlanMode, ExitPlanMode
---

# /nit:plan — the plan is a reviewable commit

This is Claude Code's plan mode with one wire moved. Plan mode runs
`EnterPlanMode` → research read-only → `ExitPlanMode`, and `ExitPlanMode`
**presents the plan to the user** for an approve/reject prompt. Here you keep
everything plan mode does — the research discipline, the complete plan, the
sign-off gate before any code — but the plan is reviewed in nit, not in the
chat. You push it as a commit (the **`lifecycle`** skill) and raise every open
question as an inline comment on it (the **`comment`** skill). The reviewer
signs off in nit; once it's approved, you implement it.

Don't enter real plan mode here — `EnterPlanMode` and `ExitPlanMode` are off,
along with `AskUserQuestion`. They can't do this job: real plan mode blocks the
writes this flow depends on (the commit, the `nit push`, the comments), and its
only exit presents to the user. This command _is_ the substitute — nit is where
the plan is reviewed and where your questions are answered, so nothing goes to
the user as a prompt.

## When this applies

Reach for `/nit:plan` whenever getting sign-off on the approach before writing
code would save a wrong-direction rewrite — the same bar as plan mode:

- a new feature, or a change to existing behavior
- several plausible approaches with no obvious winner (if each is independently
  worth _building_, that's `/nit:fork`, not a plan)
- an architectural decision, or a change spanning more than a couple of files
- requirements you can only pin down after exploring the code

Skip it when plan mode would be overkill: a one-line or obvious fix, a single
function with clear requirements, or a task the user already spelled out in
detail. Pure research — "find where X happens", "explain this module" — isn't a
plan; just answer.

## 1. Research (read-only)

Explore and form the plan the way plan mode does: read widely, learn the
existing patterns and architecture, and settle on an approach. Don't write
implementation code yet — the only artifact at this stage is the plan document.
The plan must be complete and unambiguous before you push it; an unknown becomes
a comment (step 3), not a hedge buried in the prose.

## 2. Push the plan as a commit

Write the plan to Markdown under `plan/` (e.g. `plan/<short-name>.md`): the
approach, the steps in order, the files you'll touch, and the trade-offs you
weighed. A plan is a complete unit of work, so `nit push` it — that puts it in
front of the reviewer, exactly where `ExitPlanMode` would have surfaced it to
the user.

## 3. Raise every question where it arises

Every place you'd have stopped to ask — an assumption, an open choice, a gap, a
"confirm?" — becomes a comment anchored to the **spot in the plan that raises
it**: the step that makes the assumption, the trade-off line you're unsure of,
the sentence naming the approach you'd swap. Leave each one unresolved.

```sh
nit comment --change <id> --file plan/<name>.md --line 12 --range 12:1-12:48 \
  -m "Assuming we keep the existing API shape here — confirm?"
```

Don't gather the questions into an "Open questions" section at the end of the
plan. A trailing list divorces each question from the thing it's about, so the
reviewer weighs in without the surrounding context — and the section quietly
becomes a dumping ground that lets the prose above it stay vague. Anchoring on
the line that raises the question is what lands the reviewer's answer exactly
where the decision lives — the whole reason the plan is reviewed in nit instead
of as a chat prompt. Only when a question genuinely isn't about any one line —
a concern about the plan as a whole — does a change-level comment become its
right home.

This is what replaces plan mode's approve/reject prompt: the reviewer reads the
plan, answers each question on the line it sits on, and approves or requests
changes — all in nit. Anchor as tightly as you can (range > line > change-level)
per the `comment` skill.

## 4. Park a monitor and wait — this is the gate

Pushing the plan and raising your questions is the move that, in plan mode,
`ExitPlanMode` makes when it presents to the user and blocks. nit doesn't block
you, so the discipline is yours: the chain is now open with nothing left to do
but hear back — exactly the state where a watcher must be running. **Don't end
the turn here.** Park a monitor on the plan's chain per the `lifecycle` skill
("Watch for feedback with a monitor") and wait for the reviewer; that parked
monitor is what replaces plan mode's approve/reject prompt. Ending the turn
with the plan pushed and nothing watching leaves the reviewer's answers landing
on nobody.

## 5. Refine, then implement — approval is _not_ a cue to land

Drive the loop with the `lifecycle` skill. When feedback arrives, refine by
amending the plan commit and pushing again; reply on each thread and resolve it.

When the chain reaches **`approved`** and your questions are resolved, the plan
is signed off — and here "signed off" means **implement it**, not land it. This
is the one place the usual "approved → land" reflex (the `lifecycle`/`land`
default) is **wrong**: a plan is a design artifact, not a shippable change.
**Never run the approve action / `land.sh` on a plan chain.** Landing the plan
by itself drops a half-finished unit onto `main` and marks the plan change
`merged` in nit — which has no unmerge, so you're left hand-resetting `main` to
dig back out.

Instead, build the implementation commit by commit through nit, stacked on the
plan commit in the same worktree (`/nit:goal` is the natural way to carry it the
rest of the way). You land only once the **implementation** is approved, per
this project's approve action — never the plan on its own.
