// A tiny in-memory implementation of the nit API. client.ts
// routes every call here when VITE_MOCK is set (via `await import("./fixtures")`),
// so the whole UI (including drafts, resolve, review submission and 409s)
// works without a backend.
//
// The canned data and the mutable store live in ./data; the pure builders in
// ./builders; the record shapes in ./store. This file is just the
// derivations (status, counts, chain state, path), the publish helpers, and
// the route dispatcher — the one public export, `mockRequest`.

import { ApiError } from "../client";
import type {
  Anchor,
  Chain,
  ChainState,
  ChangeDetail,
  ChangeStatus,
  CommentInput,
  NewDraft,
  Decision,
  Line,
  PathEntry,
  Repo,
  Review,
  Revision,
  DraftDecision,
  TagList,
  Verdict,
} from "../types";
import { placementLine } from "../../lib/comments";
import { verdictStatus } from "../verdict";
import { changeDetail as foldDetail } from "../fold";
import { mockAppend, projection } from "./stream";
import { diffKey, newSideEnd } from "./builders";
import { changes, draftReviews, drafts, repos, tips } from "./data";
import type {
  AuthoredFile,
  ChangeRecord,
  DraftRecord,
  TipRecord,
} from "./store";

let nextDraftId = 200;
let nextThreadId = 300;
let nextReviewId = 50;

/** Drain a change's comment drafts into `CommentInput`s for the review's log
 * entry; a new thread gets a record id stamped on so the websocket fold mints
 * the same id. The fold owns the thread, so
 * nothing is mirrored here — the review_id is attached when the entry folds. */
function drainComments(c: ChangeRecord): CommentInput[] {
  const comments: CommentInput[] = [];
  const changeDrafts = drafts
    .filter((x) => x.change_number === c.id)
    .sort((a, b) => a.id - b.id);
  for (const d of changeDrafts) {
    if (d.thread_id !== null) {
      // A reply: the folded thread owns the anchor, so only id/body/resolved.
      comments.push({
        thread_id: d.thread_id,
        revision: null,
        anchor: null,
        body: d.body,
        resolved: d.resolved,
      });
    } else if (d.body.trim() !== "") {
      comments.push({
        thread_id: nextThreadId++,
        revision: d.revision,
        anchor: d.anchor,
        body: d.body,
        resolved: d.resolved,
      });
    }
    drafts.splice(drafts.indexOf(d), 1);
  }
  return comments;
}

/** Why a draft decision can't publish against the change's lifecycle, or null
 * (mirrors the server's decision_block). */
function decisionBlock(c: ChangeRecord, decision: Decision): string | null {
  if (c.terminal === "merged") return "change is merged — nothing to submit";
  if (c.terminal === "abandoned") {
    return decision === "reopen"
      ? null
      : "change is abandoned — draft Reopen first";
  }
  return decision === "reopen"
    ? "change is live — Reopen does not apply"
    : null;
}

/** Publish one draft decision (mirrors the server's publish_member): an
 * optional reopen, a review draining comment drafts (the decision's verdict, or
 * `comment` to carry draft comments under a lifecycle decision), then an
 * optional abandon. */
function publishMember(
  c: ChangeRecord,
  decision: Decision,
  message: string,
  revision: number,
  now: string,
): void {
  if (decision === "reopen") {
    c.terminal = undefined;
    emitLifecycle(c.id, now, "reopened");
  }
  const hasComments = drafts.some((d) => d.change_number === c.id);
  const verdict: Verdict | null =
    decision === "approve" ||
    decision === "request_changes" ||
    decision === "comment"
      ? decision
      : hasComments
        ? "comment"
        : null;
  if (verdict) {
    const review: Review = {
      id: nextReviewId++,
      revision,
      verdict,
      message: decision === verdict ? message : "",
      created_at: now,
    };
    c.reviews.push(review);
    const comments = drainComments(c);
    mockAppend(c.id, now, {
      kind: "review",
      payload: {
        revision,
        verdict,
        message: review.message,
        comments,
      },
    });
  }
  if (decision === "abandon" && !c.terminal) {
    c.terminal = "abandoned";
    emitLifecycle(c.id, now, "abandoned");
  }
}

/** Append a `lifecycle` entry to a change's mock log so live followers see the
 * abandon/reopen the record mutation just made. */
function emitLifecycle(
  changeNumber: number,
  now: string,
  action: "abandoned" | "reopened",
): void {
  mockAppend(changeNumber, now, {
    kind: "lifecycle",
    payload: { action, message: null },
  });
}

// ---------------------------------------------------------------------------
// Derivations (status, counts, chain state, path) so mutations stay consistent

/** The commit-sha → (change, revision) index — the basis for the SHA-walk
 * that derives every chain path. */
const shaIndex = new Map<
  string,
  { change: ChangeRecord; revision: Revision }
>();
for (const c of changes) {
  for (const r of c.revisions)
    shaIndex.set(r.commit_sha, { change: c, revision: r });
}

const latestRevision = (c: ChangeRecord): Revision => {
  const r = c.revisions[c.revisions.length - 1];
  if (!r) throw new Error(`change ${c.id} has no revisions`);
  return r;
};

/** A change's displayed status at a given revision: terminal wins; else
 * the verdict of the latest review at that revision, falling back to
 * pending. */
function statusAt(c: ChangeRecord, revision: number): ChangeStatus {
  if (c.terminal) return c.terminal;
  const review = c.reviews
    .filter((r) => r.revision === revision)
    .sort((a, b) => a.id - b.id)
    .at(-1);
  if (!review) return "pending";
  return verdictStatus[review.verdict];
}

/** Walk a tip back to base through parent_sha, oldest-first (base → tip).
 * Each member pins the revision the tip walked through (the sha in the
 * index); the walk stops at a parent_sha that is no change (the merge-base
 * on the canonical ref). */
function walkPath(
  tip: TipRecord,
): { change: ChangeRecord; revision: Revision }[] {
  const tipChange = changes.find((c) => c.id === tip.tip_change_number);
  if (!tipChange)
    throw new Error(`unknown tip change ${tip.tip_change_number}`);
  const tipRev =
    tipChange.revisions.find((r) => r.number === tip.revision) ??
    latestRevision(tipChange);
  const out: { change: ChangeRecord; revision: Revision }[] = [
    { change: tipChange, revision: tipRev },
  ];
  let parent = tipRev.parent_sha;
  for (
    let member = shaIndex.get(parent);
    member !== undefined;
    member = shaIndex.get(parent)
  ) {
    out.push(member);
    parent = member.revision.parent_sha;
  }
  return out.reverse();
}

function pathEntry(
  member: { change: ChangeRecord; revision: Revision },
  position: number,
): PathEntry {
  const { change: c, revision } = member;
  return {
    change_number: c.id,
    position,
    change_id: c.change_id,
    revision: revision.number,
    status: statusAt(c, revision.number),
    subject: c.subject,
    commit_sha: revision.commit_sha,
  };
}

function derivePath(tip: TipRecord): PathEntry[] {
  return walkPath(tip).map((m, i) => pathEntry(m, i));
}

/** Mirrors the server's chain-state rollup: abandoned members are dropped
 * before it. */
function chainState(path: PathEntry[]): ChainState {
  const live = path.filter((e) => e.status !== "abandoned");
  if (live.length === 0) return "authors_turn";
  if (live.every((e) => e.status === "merged")) return "merged";
  if (
    live.some(
      (e) => e.status === "changes_requested" || e.status === "commented",
    )
  ) {
    return "authors_turn";
  }
  if (live.some((e) => e.status === "pending")) return "waiting_for_review";
  // The rest are approved (≥1) and/or merged, no pending — approved.
  return "approved";
}

function chainView(tip: TipRecord): Chain {
  const path = derivePath(tip);
  return {
    tip_change_number: tip.tip_change_number,
    repo_id: tip.repo_id,
    state: chainState(path),
    path,
  };
}

/** Resolve `GET /chains/{change_number}?revision=N` to a tip (mirrors the backend's
 * `tip_for`): a live tip whose path walks `changeNumber` at that revision, else the
 * change as its own degenerate tip. So an INTERIOR change resolves to the tip
 * that extends through it (the full chain), not a 404. */
function resolveTip(
  changeNumber: number,
  requested?: number,
): TipRecord | undefined {
  const c = changes.find((x) => x.id === changeNumber);
  if (!c) return undefined;
  const revision = requested ?? latestRevision(c).number;
  for (const tip of tips) {
    const member = derivePath(tip).find(
      (e) => e.change_number === changeNumber,
    );
    if (member?.revision === revision) return tip;
  }
  return {
    tip_change_number: changeNumber,
    repo_id: c.repo_id,
    revision,
    active: !c.terminal,
  };
}

/** Derive the repo registry (`GET /api/repos`). `active_chains`
 * is the live tip count for the repo. */
function repoList(): Repo[] {
  return repos.map((r) => ({
    id: r.id,
    git_dir: r.git_dir,
    canonical_ref: r.canonical_ref,
    active_chains: tips.filter((t) => t.repo_id === r.id && t.active).length,
  }));
}

// ---------------------------------------------------------------------------
// The graph's two primitive reads. The browser assembles the graph itself
// (api/graph, the shared wasm derivation), so the mock serves only parts:
// each repo's change folds and a window of its synthetic canonical history.

/** The fixed merged-history window (mirrors the backend's MERGED_WINDOW). */
const MERGED_WINDOW = 5;

/** `GET /api/changes`: folded projections of the repo's changes matching the
 * explicit `status` filters (none means all), each folded from its synth log
 * through the shared wasm fold — the same source the websocket projections. */
function listChanges(repoId: number | null, statuses: ChangeStatus[]) {
  return {
    changes: changes
      .filter(
        (c) =>
          (repoId === null || c.repo_id === repoId) &&
          (statuses.length === 0 ||
            statuses.includes(statusAt(c, latestRevision(c).number))),
      )
      .map((c) => projection(c.id)),
  };
}

/** `GET /api/tags`: the tags the repo's changes at `statuses` (none means
 * all) carry, each key with its sorted distinct values. */
function listTags(repoId: number, statuses: ChangeStatus[]): TagList {
  const tags: Record<string, string[]> = {};
  for (const c of changes) {
    if (c.repo_id !== repoId) continue;
    if (
      statuses.length > 0 &&
      !statuses.includes(statusAt(c, latestRevision(c).number))
    )
      continue;
    for (const [key, value] of Object.entries(c.tags ?? {})) {
      const values = (tags[key] ??= []);
      if (!values.includes(value)) values.push(value);
    }
  }
  for (const values of Object.values(tags)) values.sort();
  return {
    tags: Object.fromEntries(
      Object.entries(tags).sort(([a], [b]) => a.localeCompare(b)),
    ),
  };
}

/** `GET /api/history`: the repo's synthetic canonical history, HEAD-first, a
 * fixed window deep. A node naming a landed change (`change_id`) is enriched
 * with it; any other commit reports both id and key null (coupled). */
function repoHistory(repoId: number) {
  const repo = repos.find((r) => r.id === repoId) ?? notFound(`repo ${repoId}`);
  const commits = repo.history.slice(0, MERGED_WINDOW + 1).map((h) => {
    const landed = h.change_id
      ? changes.find((c) => c.repo_id === repoId && c.change_id === h.change_id)
      : undefined;
    return {
      sha: h.sha,
      parents: h.parents,
      subject: h.subject,
      change_number: landed?.id ?? null,
      change_id: landed?.change_id ?? null,
    };
  });
  return { commits, truncated: repo.history.length > MERGED_WINDOW + 1 };
}

// The published view (revisions/threads/reviews) folds the change's single
// synth log — the same source the websocket projection folds — so a mutation that
// appends to the log shows up identically over REST and the stream. The
// reviewer's drafts and draft decision are not log state, so overlay them.
function changeDetail(c: ChangeRecord): ChangeDetail {
  return { ...foldDetail(projection(c.id)), ...changeDrafts(c) };
}

/** The reviewer's overlay alone (`GET /changes/{id}/drafts`). */
function changeDrafts(c: ChangeRecord) {
  return {
    drafts: drafts.filter((x) => x.change_number === c.id),
    draft_decision: draftReviews.get(c.id) ?? null,
  };
}

/** Find the text of a diff line so new drafts get a line_text projection. */
function snapshotLineText(
  c: ChangeRecord,
  revision: number,
  anchor: Anchor,
): Anchor {
  if (anchor === "change" || "file" in anchor) return anchor;
  const { file, side } = anchor.line;
  const line = placementLine(anchor.line.at);
  const diff = c.diffs[diffKey(revision)];
  const f = diff?.files.find((x) => x.path === file || x.old_path === file);
  const lines = f?.hunks.flatMap((hunk) => hunk.lines) ?? [];
  const hit = lines.find((l) => (side === "new" ? l.new : l.old) === line);
  return { line: { ...anchor.line, line_text: hit?.text } };
}

/** Reconstruct the whole file as diff lines from its shown hunks,
 * filling the gaps between, above, and below them with synthesized context.
 * The mock has no real file bodies, so this is what `/lines` returns. */
function wholeLines(file: AuthoredFile): Line[] {
  const out: Line[] = [];
  let oldN = 1;
  let newN = 1;
  const fill = (until: number) => {
    while (newN < until) {
      out.push({
        kind: "context",
        old: oldN,
        new: newN,
        text: `    // unchanged line ${newN}`,
      });
      oldN++;
      newN++;
    }
  };
  for (const hunk of file.hunks) {
    fill(hunk.new_start);
    for (const l of hunk.lines) {
      out.push(l);
      if (l.old !== undefined) oldN = l.old + 1;
      if (l.new !== undefined) newN = l.new + 1;
    }
  }
  fill(newSideEnd(file) + 1);
  return out;
}

const notFound = (what: string): never => {
  throw new ApiError(404, `${what} not found`);
};

const getChange = (id: number): ChangeRecord =>
  changes.find((c) => c.id === id) ?? notFound(`change ${id}`);

// ---------------------------------------------------------------------------
// The mock router — one arm per server endpoint

const LATENCY_MS = 40;

export async function mockRequest(
  method: string,
  path: string,
  body?: unknown,
): Promise<unknown> {
  await new Promise((r) => setTimeout(r, LATENCY_MS));
  const url = new URL(path, "http://mock");
  const p = url.pathname;
  const q = url.searchParams;
  let m: RegExpExecArray | null;

  if (method === "GET" && p === "/repos") {
    return { repos: repoList() };
  }

  if ((m = /^\/repos\/(\d+)$/.exec(p)) && method === "GET") {
    const id = Number(m[1]);
    return repoList().find((r) => r.id === id) ?? notFound(`repo ${id}`);
  }

  if (method === "GET" && p === "/changes") {
    const repo = q.get("repo");
    return listChanges(
      repo === null ? null : Number(repo),
      q.getAll("status") as ChangeStatus[],
    );
  }

  if (method === "GET" && p === "/history") {
    return repoHistory(Number(q.get("repo")));
  }

  if (method === "GET" && p === "/tags") {
    return listTags(
      Number(q.get("repo")),
      q.getAll("status") as ChangeStatus[],
    );
  }

  if (method === "GET" && p === "/chains") {
    const status = q.get("status") ?? "active";
    const repo = q.get("repo");
    const listed = tips.filter(
      (t) =>
        (status === "all" || t.active) &&
        (repo === null || t.repo_id === Number(repo)),
    );
    return { chains: listed.map(chainView) };
  }

  // The aggregated chain log is not in this cut (events return later); serve
  // an empty timeline so the endpoint exists.
  if ((m = /^\/chains\/(\d+)\/log$/.exec(p)) && method === "GET") {
    const id = Number(m[1]);
    if (!tips.some((t) => t.tip_change_number === id))
      return notFound(`chain ${id}`);
    return { entries: [] };
  }

  if ((m = /^\/chains\/(\d+)$/.exec(p)) && method === "GET") {
    const id = Number(m[1]);
    const revision = q.has("revision") ? Number(q.get("revision")) : undefined;
    const tip = resolveTip(id, revision);
    if (!tip) return notFound(`chain ${id}`);
    return chainView(tip);
  }

  // Batch submit.
  if ((m = /^\/chains\/(\d+)\/submit$/.exec(p)) && method === "POST") {
    const id = Number(m[1]);
    const revision = q.has("revision") ? Number(q.get("revision")) : undefined;
    const tip = resolveTip(id, revision);
    if (!tip) return notFound(`chain ${id}`);
    const now = new Date().toISOString();
    let submitted = 0;
    const errors: { change_number: number; message: string }[] = [];
    for (const member of derivePath(tip)) {
      const draft = draftReviews.get(member.change_number);
      if (!draft) continue; // no decision — leave the member's comment drafts
      const c = changes.find((x) => x.id === member.change_number);
      if (!c) continue;
      const block = decisionBlock(c, draft.decision);
      if (block) {
        errors.push({ change_number: c.id, message: block });
        continue;
      }
      publishMember(c, draft.decision, draft.message, member.revision, now);
      draftReviews.delete(c.id);
      submitted++;
    }
    return { submitted, errors };
  }

  if ((m = /^\/changes\/(\d+)$/.exec(p)) && method === "GET") {
    return changeDetail(getChange(Number(m[1])));
  }

  if ((m = /^\/changes\/(\d+)\/drafts$/.exec(p)) && method === "GET") {
    return changeDrafts(getChange(Number(m[1])));
  }

  if (
    (m = /^\/changes\/(\d+)\/revisions\/(\d+)\/diff$/.exec(p)) &&
    method === "GET"
  ) {
    const c = getChange(Number(m[1]));
    const number = Number(m[2]);
    const against = q.has("against") ? Number(q.get("against")) : undefined;
    const revision = c.revisions.find((r) => r.number === number);
    if (!revision) notFound(`revision ${number}`);
    const diff = c.diffs[diffKey(number, against)];
    if (!diff) return notFound(`diff for revision ${number}`);
    // Fill the EOF anchor the wire shape carries but ./data omits.
    const files = diff.files.map((f) => ({ ...f, new_total: newSideEnd(f) }));
    return structuredClone({ files });
  }

  // Context expansion. The fixtures hold diffs, not whole files, so
  // reconstruct the whole file from the
  // shown hunks with synthesized context filling the gaps — enough for the
  // expand controls to reveal rows. (Real drift in a gap is the backend's
  // job; the mock just makes the interaction renderable.)
  if (
    (m = /^\/changes\/(\d+)\/revisions\/(\d+)\/lines$/.exec(p)) &&
    method === "GET"
  ) {
    const c = getChange(Number(m[1]));
    const revision = Number(m[2]);
    const against = q.has("against") ? Number(q.get("against")) : undefined;
    const path = q.get("path") ?? "";
    const file = c.diffs[diffKey(revision, against)]?.files.find(
      (f) => f.path === path,
    );
    return { lines: file ? wholeLines(file) : [] };
  }

  if ((m = /^\/changes\/(\d+)\/drafts$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    const req = body as NewDraft;
    const now = new Date().toISOString();
    const record: DraftRecord = {
      id: nextDraftId++,
      change_number: c.id,
      thread_id: req.thread_id ?? null,
      revision: req.revision,
      anchor: snapshotLineText(c, req.revision, req.anchor ?? "change"),
      body: req.body,
      resolved: req.resolved ?? false,
      created_at: now,
      updated_at: now,
    };
    drafts.push(record);
    return record;
  }

  if ((m = /^\/drafts\/(\d+)$/.exec(p)) && method === "PATCH") {
    const id = Number(m[1]);
    const d = drafts.find((x) => x.id === id);
    if (!d) return notFound(`draft ${id}`);
    const req = body as { body: string; resolved?: boolean };
    d.body = req.body;
    if (req.resolved !== undefined) d.resolved = req.resolved;
    d.updated_at = new Date().toISOString();
    return d;
  }

  if ((m = /^\/drafts\/(\d+)$/.exec(p)) && method === "DELETE") {
    const id = Number(m[1]);
    const i = drafts.findIndex((x) => x.id === id);
    if (i < 0) notFound(`draft ${id}`);
    drafts.splice(i, 1);
    return undefined;
  }

  // Draft / clear a reviewer decision (drafted like a comment; published by
  // the chain batch submit above).
  if ((m = /^\/changes\/(\d+)\/decision$/.exec(p)) && method === "PUT") {
    const c = getChange(Number(m[1]));
    const req = body as DraftDecision;
    const draft = { decision: req.decision, message: req.message };
    draftReviews.set(c.id, draft);
    return draft;
  }

  if ((m = /^\/changes\/(\d+)\/decision$/.exec(p)) && method === "DELETE") {
    const c = getChange(Number(m[1]));
    draftReviews.delete(c.id);
    return undefined;
  }

  if ((m = /^\/changes\/(\d+)\/abandon$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    if (!c.terminal) {
      c.terminal = "abandoned";
      emitLifecycle(c.id, new Date().toISOString(), "abandoned");
    }
    return changeDetail(c);
  }

  if ((m = /^\/changes\/(\d+)\/reopen$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    if (c.terminal === "abandoned") {
      c.terminal = undefined;
      emitLifecycle(c.id, new Date().toISOString(), "reopened");
    }
    return changeDetail(c);
  }

  throw new ApiError(404, `mock: no route for ${method} ${path}`);
}
