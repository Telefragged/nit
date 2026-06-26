---
name: plan
description: Plan-mode for nit — research read-only, then instead of presenting the plan to the user, push it as a reviewable commit (plan/<name>.md) and raise every question you'd have asked as an inline comment on the plan. Implement once it's approved.
disallowed-tools: AskUserQuestion
---

# /nit:plan — the plan is a reviewable commit

Claude Code's plan mode, redirected through nit. You research and form a plan
as usual — but you don't present it. You push the plan as a commit (the
**`lifecycle`** skill) and raise every question as an inline comment on it (the
**`comment`** skill). The user reviews the plan in nit; once it's approved, you
implement it.

`AskUserQuestion` is off here. Open questions go **into the plan as comments**,
anchored to the line they concern — never to the user as a prompt.

## 1. Research (read-only)

Explore and form the plan the way plan mode does. Don't write implementation
code yet — at this stage the only thing you produce is the plan document.

## 2. Push the plan as a commit

Write the plan to Markdown, preferably under `plan/` (e.g.
`plan/<short-name>.md`): the approach, the steps, the files you'll touch, the
trade-offs. A plan is a complete unit, so `nit push` it.

## 3. Raise every question inline

For each thing you'd have asked — an assumption, an open choice, a gap — open a
comment **on the exact line or range of the plan**, left unresolved:

```sh
nit comment --change <id> --file plan/<name>.md --line 12 --range 12:1-12:48 \
  -m "Assuming we keep the existing API shape here — confirm?"
```

This replaces plan mode's approve/reject prompt: the user reviews the plan,
answers your questions on the lines they sit on, and approves or requests
changes — all in nit.

## 4. Refine, then implement

Drive the loop with the `lifecycle` skill. On feedback, refine by amending the
plan commit and pushing again; reply on each thread and resolve it. When the
chain reaches **`approved`** and your questions are resolved, the plan is signed
off — **now implement it**, building it out commit by commit through nit
(`/nit:goal` is the natural way). Until then, the only thing committed is the
plan document.
