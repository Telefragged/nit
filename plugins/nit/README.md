# nit — agent plugin

A Claude Code plugin that teaches agents to drive code review through
[nit](https://github.com/Telefragged/nit). Instead of landing work directly or
stalling on open questions, agents push each completed commit for review and
keep the whole conversation — decisions, questions, alternatives — inside nit,
anchored to the exact lines it concerns.

## Install

`nit install-plugin` installs the plugin that your `nit` binary carries, so
the plugin always matches your nit version. It registers the plugin with
Claude Code as `nit@nit`.

Without nit on the machine, install from the
[nit repo](https://github.com/Telefragged/nit). It is also a Claude Code
plugin marketplace. Add it, then install the plugin:

```
/plugin marketplace add Telefragged/nit
/plugin install nit@nit
```

`/plugin list` confirms it's enabled and the `/nit:*` skills and commands are
available; pin a branch or tag with `Telefragged/nit@<ref>`. Then run
`/nit:install` once to set nit up for your project (see Prerequisites).

## Skills — the base behavior

These drive nit on their own. Once the plugin is enabled, an agent can take a
change through review from these alone — e.g. when you say "drive it through
nit".

| Skill            | What it does                                                                                                                                                       |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `/nit:lifecycle` | Drive a change through the review loop — push each completed commit, act on the reviewer, answer feedback by amending in place, and land once approved.            |
| `/nit:comment`   | Talk to the reviewer in nit — open and reply to threads with `nit comment`, anchored as tightly as possible, keeping the conversation in nit rather than the chat. |

## The watch — how the reviewer reaches the agent

The agent starts `nit watch` in the background at the start of a session, the
way the `/nit:lifecycle` skill tells it to. Every review, comment and lifecycle
change from the reviewer then arrives in that session as a message, and the
agent acts on it without a prompt from you.

`nit watch` follows the changes the session pushed and posts each review to the
session's own
[inbox socket](https://code.claude.com/docs/en/cross-session-messaging#the-sessions-inbox-socket).
It runs as a child of the session, so it lives exactly as long as the session
does, however long that session idles.

## Commands — things you initiate

User-invoked workflows built on the skills above.

| Command        | What it does                                                                                                                                                                      |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/nit:install` | Set nit up for this project: make the `nit` CLI reachable, register the repo, ensure commits get a Change-Id, and record the project's approve action in `CLAUDE.md`/`AGENTS.md`. |
| `/nit:goal`    | Pursue a goal to completion without blocking on questions — decide, implement, and annotate the call on the exact lines for the reviewer, then keep going.                        |
| `/nit:fork`    | Facing several viable approaches? Fork into parallel sub-agents, implement each variation, and push them all as separate changes to compare.                                      |
| `/nit:plan`    | Push the plan as a reviewable commit and raise every open question as an inline comment, then implement once it's approved.                                                       |

`/nit:goal`, `/nit:fork`, and `/nit:plan` turn off the question prompt: an
unclear point becomes an annotation on the code or plan, not an interruption.
`/nit:install` is the one command that asks the user directly.

## Prerequisites

- The `nit` CLI on `PATH`, or invoked as `nix run github:Telefragged/nit -- <args>`.
- A reachable nit server (`$NIT_SERVER`, default `http://127.0.0.1:8877`).

Run `/nit:install` once to set both up.
