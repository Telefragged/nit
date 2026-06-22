---
name: fork
description: When a task has several viable approaches and no clear winner, fork into parallel sub-agents (at most 3 per decision, at most 2 levels deep), implement each variation, and push them all to nit as separate changes for the reviewer to compare.
disallowed-tools: AskUserQuestion
---

# /nit:fork — build the alternatives in parallel

An opinionated way to drive a change through nit. Each variation is driven with
the **`lifecycle`** skill (push → monitor → land) and annotated with the
**`comment`** skill. This command adds the forking rule.

When you face a real fork — several approaches each worth building, with no
obvious winner — don't ask and don't silently pick one. **Fork yourself**:
spawn sub-agents that each implement one variation and push it to nit as its
own change. The reviewer compares finished alternatives.

`AskUserQuestion` is off here. Forking _is_ how you answer "which option?"; a
smaller unclear call inside one variation is handled the `/nit:goal` way —
decide and annotate with `nit comment`.

## When the fork appears

- **Upfront** — you see the divergence before you start. Implement everything
  the variations _share_ first, then fork at the divergence point so each
  variation builds on the same base.
- **Mid-stream** — you only realise partway through that the path splits. Fork
  from where you are; the work already done is the shared base.

## Forking rules

- **At most 3 sub-agents per decision point.** More than three candidates: pick
  the three most distinct and note the ones you dropped (a comment on one
  variation).
- **At most 2 levels of recursion.** A sub-agent may fork once more; its
  children may not. Pass the remaining depth in each sub-agent's prompt. At
  depth 0, a sub-agent that hits another fork stops forking and falls back to
  `/nit:goal`.
- **Every variation gets pushed** as its own change — push all of them, not
  just the one you like best.

## How to fork

Spawn one sub-agent per variation with the **Agent tool**, each in its own git
worktree (`isolation: "worktree"`). Give each sub-agent: the shared context and
the common base, the one variation it owns, its fork budget, and standing
instructions to implement the variation, drive it through nit (the `lifecycle`
skill), annotate the trade-off it represents (the `comment` skill, left
unresolved), not ask questions, and fork again only within budget. Launch them
in parallel (one message, multiple Agent calls).

## After forking

Each variation lives as its own pushed change, with its trade-off annotated in
nit — that's where the comparison happens. Send the conversation only a brief
note: the set of variations and where each lives. Don't merge them together or
pre-select one; the reviewer decides which survives.
