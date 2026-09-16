// A tiny in-memory implementation of the nit API. client.ts
// routes every call here when VITE_MOCK is set (via `await import("./fixtures")`),
// so the whole UI (including drafts, resolve, review submission and 409s)
// works without a backend.
//
// The canned data and the mutable store live in ./data; the pure builders in
// ./builders; the record shapes in ./store. This file is just the
// derivations (status, counts), the publish helpers, and
// the route dispatcher — the one public export, `mockRequest`.

import { ApiError } from "../client";
import type {
  Anchor,
  ChangeDetail,
  ChangeQuery,
  ChangeStatus,
  CommentInput,
  NewDraft,
  Decision,
  Line,
  Repo,
  Review,
  DraftDecision,
  TagList,
  Verdict,
} from "../types";
import { placementLine } from "../../lib/comments";
import { changeDetail as foldDetail } from "../fold";
import { mockAppend, picks, projection } from "./stream";
import { diffKey, sideEnd } from "./builders";
import { changes, draftReviews, drafts, repos } from "./data";
import type {
  AuthoredFile,
  AuthoredRevision,
  ChangeRecord,
  DraftRecord,
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

/** Publish one draft decision (mirrors the server's publish_change): an
 * optional reopen, a review draining comment drafts (the decision's verdict, or
 * `comment` to carry draft comments under a lifecycle decision), then an
 * optional abandon. */
function publishChange(
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
// Derivations (status, counts) so mutations stay consistent

const latestRevision = (c: ChangeRecord): AuthoredRevision => {
  const r = c.revisions[c.revisions.length - 1];
  if (!r) throw new Error(`change ${c.id} has no revisions`);
  return r;
};

/** Derive the repo registry (`GET /api/repos`). `open_changes` counts the
 * repo's changes that are neither merged nor abandoned. */
function repoList(): Repo[] {
  return repos.map((r) => ({
    id: r.id,
    git_dir: r.git_dir,
    canonical_ref: r.canonical_ref,
    open_changes: changes.filter((c) => c.repo_id === r.id && !c.terminal)
      .length,
  }));
}

// ---------------------------------------------------------------------------
// The graph's two primitive reads. The browser assembles the graph itself
// (api/graph, the shared wasm derivation), so the mock serves only parts:
// each repo's change folds and a window of its synthetic canonical history.

/** The fixed merged-history window (mirrors the backend's MERGED_WINDOW). */
const MERGED_WINDOW = 5;

/** `GET /api/changes`: the folded projections of the changes `query` picks,
 * each folded from its synth log through the shared wasm fold — the same
 * source as the websocket projections. */
function listChanges(query: ChangeQuery) {
  return {
    changes: changes
      .filter((c) => picks(query, c))
      .map((c) => projection(c.id)),
  };
}

/** `GET /api/tags`: the tags the repo's changes at `statuses` (none means
 * all) carry, each key with its sorted distinct values. The same read as
 * `GET /api/changes` picks the changes, so the two routes agree. */
function listTags(repoId: number, statuses: ChangeStatus[]): TagList {
  const tags: Record<string, string[]> = {};
  for (const p of listChanges({ repo: repoId, status: statuses }).changes) {
    for (const [key, value] of Object.entries(p.tags ?? {})) {
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
  fill(sideEnd(file, "new") + 1);
  return out;
}

/** A change query read back from its query string. */
function changeQuery(q: URLSearchParams): ChangeQuery {
  const repo = q.get("repo");
  const change = q.get("change");
  return {
    repo: repo === null ? undefined : Number(repo),
    status: q.getAll("status") as ChangeStatus[],
    tag: q.getAll("tag"),
    change: change === null ? undefined : Number(change),
    change_id: q.get("change_id") ?? undefined,
  };
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
    return listChanges(changeQuery(q));
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

  // Batch submit: every draft decision the change query picks, each at the
  // change's latest revision.
  if (method === "POST" && p === "/submit") {
    const picked = listChanges(changeQuery(q)).changes;
    const now = new Date().toISOString();
    let submitted = 0;
    const errors: { change_number: number; message: string }[] = [];
    for (const { id } of picked) {
      const draft = draftReviews.get(id);
      if (!draft) continue; // no decision — leave the change's comment drafts
      const c = getChange(id);
      const block = decisionBlock(c, draft.decision);
      if (block) {
        errors.push({ change_number: c.id, message: block });
        continue;
      }
      publishChange(
        c,
        draft.decision,
        draft.message,
        latestRevision(c).number,
        now,
      );
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
    const revision = c.revisions[number];
    if (!revision) notFound(`revision ${number}`);
    const diff = c.diffs[diffKey(number, against)];
    if (!diff) return notFound(`diff for revision ${number}`);
    // Fill the EOF anchors the wire shape carries but ./data omits.
    const files = diff.files.map((f) => ({
      ...f,
      old_total: sideEnd(f, "old"),
      new_total: sideEnd(f, "new"),
    }));
    return structuredClone({ files });
  }

  if (
    (m = /^\/changes\/(\d+)\/revisions\/(\d+)\/ported$/.exec(p)) &&
    method === "GET"
  ) {
    const c = getChange(Number(m[1]));
    const number = Number(m[2]);
    if (!c.revisions[number]) notFound(`revision ${number}`);
    const against = q.has("against") ? Number(q.get("against")) : undefined;
    const at = (revision: number | undefined) =>
      revision === undefined ? [] : (c.ported?.[revision] ?? []);
    const ported = [...at(number), ...at(against)];
    if (q.get("include_resolved") === "true") return structuredClone(ported);
    const resolved = new Set(
      projection(c.id)
        .threads.filter((t) => t.resolved)
        .map((t) => t.id),
    );
    return structuredClone(ported.filter((p) => !resolved.has(p.thread_id)));
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
  // the batch submit above).
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
