// A tiny in-memory implementation of the API from docs/api.md. client.ts
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
  Chain,
  ChainRef,
  ChainState,
  ChainSummary,
  ChangeDetail,
  ChangeStatus,
  CommentSide,
  CreateDraftRequest,
  Decision,
  Draft,
  GraphNode,
  PathEntry,
  Repo,
  RepoGraph,
  Review,
  Revision,
  StageDecisionRequest,
  Thread,
  Verdict,
} from "../types";
import { diffKey } from "./builders";
import {
  changes,
  draftReviews,
  drafts,
  graphHistory,
  graphScenarios,
  repos,
  threads,
  tips,
} from "./data";
import type {
  ChangeRecord,
  DraftRecord,
  ThreadRecord,
  TipRecord,
} from "./store";

let nextDraftId = 200;
let nextThreadId = 300;
let nextReviewId = 50;

/** Drain a change's comment drafts into `review`, opening or updating their
 * threads; returns the threads it touched. Shared by the immediate POST
 * /reviews and the batch submit (docs/api.md "Thread resolution"). */
function drainComments(
  c: ChangeRecord,
  review: Review,
  now: string,
): ThreadRecord[] {
  const touched = new Map<number, ThreadRecord>();
  const changeDrafts = drafts
    .filter((x) => x.change_id === c.id)
    .sort((a, b) => a.id - b.id);
  for (const d of changeDrafts) {
    const hasBody = d.body.trim() !== "";
    if (d.thread_id !== null) {
      const thread = threads.find((x) => x.id === d.thread_id);
      if (thread) {
        thread.resolved = d.resolved;
        thread.updated_at = now;
        if (hasBody) {
          thread.comments.push({
            author: "reviewer",
            body: d.body,
            review_id: review.id,
            created_at: now,
          });
        }
        touched.set(thread.id, thread);
      }
    } else if (hasBody) {
      const thread: ThreadRecord = {
        id: nextThreadId++,
        change_id: c.id,
        revision: d.revision,
        file: d.file,
        line: d.line,
        side: d.side,
        range: d.range ?? null,
        line_text: d.line_text,
        resolved: d.resolved,
        comments: [
          {
            author: "reviewer",
            body: d.body,
            review_id: review.id,
            created_at: now,
          },
        ],
        created_at: now,
        updated_at: now,
      };
      threads.push(thread);
      touched.set(thread.id, thread);
    }
    drafts.splice(drafts.indexOf(d), 1);
  }
  return [...touched.values()];
}

/** Why a staged decision can't publish against the change's lifecycle, or null
 * (mirrors the server's decision_block). */
function decisionBlock(c: ChangeRecord, decision: Decision): string | null {
  if (c.terminal === "merged") return "change is merged — nothing to submit";
  if (c.terminal === "abandoned") {
    return decision === "reopen"
      ? null
      : "change is abandoned — stage Reopen first";
  }
  return decision === "reopen"
    ? "change is live — Reopen does not apply"
    : null;
}

/** Publish one staged decision (mirrors the server's publish_member): an
 * optional reopen, a review draining comment drafts (the decision's verdict, or
 * `comment` to carry staged comments under a lifecycle decision), then an
 * optional abandon. */
function publishMember(
  c: ChangeRecord,
  decision: Decision,
  message: string,
  revision: number,
  now: string,
): void {
  if (decision === "reopen") c.terminal = undefined;
  const hasComments = drafts.some((d) => d.change_id === c.id);
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
    drainComments(c, review, now);
  }
  if (decision === "abandon") c.terminal ??= "abandoned";
}

// ---------------------------------------------------------------------------
// Derivations (status, counts, chain state, path) so mutations stay consistent

/** The commit-sha → (change, revision) index — the basis for the SHA-walk
 * that derives every chain path (docs/api.md "Chains"). */
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

/** A change's displayed status at a given revision (docs/api.md "State
 * table"): terminal wins; else the verdict of the latest review at that
 * revision, falling back to pending. */
function statusAt(c: ChangeRecord, revision: number): ChangeStatus {
  if (c.terminal) return c.terminal;
  const review = c.reviews
    .filter((r) => r.revision === revision)
    .sort((a, b) => a.id - b.id)
    .at(-1);
  if (!review) return "pending";
  const byVerdict: Record<Verdict, ChangeStatus> = {
    approve: "approved",
    request_changes: "changes_requested",
    comment: "commented",
  };
  return byVerdict[review.verdict];
}

/** Walk a tip back to base through parent_sha, oldest-first (base → tip).
 * Each member pins the revision the tip walked through (the sha in the
 * index); the walk stops at a parent_sha that is no change (the merge-base
 * on the canonical branch). */
function walkPath(
  tip: TipRecord,
): { change: ChangeRecord; revision: Revision }[] {
  const tipChange = changes.find((c) => c.id === tip.tip_change_id);
  if (!tipChange) throw new Error(`unknown tip change ${tip.tip_change_id}`);
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
  return out.reverse(); // base → tip
}

function pathEntry(
  member: { change: ChangeRecord; revision: Revision },
  position: number,
): PathEntry {
  const { change: c, revision: rev } = member;
  const ownThreads = threads.filter(
    (t) => t.change_id === c.id && t.revision === rev.number,
  );
  const ownDrafts = drafts.filter(
    (d) => d.change_id === c.id && d.revision === rev.number,
  );
  const latest = latestRevision(c).number;
  return {
    change_id: c.id,
    position,
    change_key: c.change_key,
    revision: rev.number,
    latest_revision: latest,
    status: statusAt(c, rev.number),
    merged_elsewhere:
      c.merged_revision !== undefined && c.merged_revision > rev.number,
    subject: c.subject,
    commit_sha: rev.commit_sha,
    counts: {
      threads: ownThreads.length,
      drafts: ownDrafts.length,
      unresolved: ownThreads.filter((t) => !t.resolved).length,
    },
    draft_decision: draftReviews.get(c.id)?.decision ?? null,
  };
}

function derivePath(tip: TipRecord): PathEntry[] {
  return walkPath(tip).map((m, i) => pathEntry(m, i));
}

/** A chain's derived state from its path members (docs/api.md state table).
 * Abandonment is derivation-inert: abandoned members are dropped before the
 * rollup, and there is no abandoned chain state. */
function chainState(tip: TipRecord, path: PathEntry[]): ChainState {
  const live = path.filter((e) => e.status !== "abandoned");
  if (live.length === 0) return "agents_turn"; // empty or all-abandoned tip
  if (live.every((e) => e.status === "merged")) return "merged";
  if (
    live.some(
      (e) => e.status === "changes_requested" || e.status === "commented",
    )
  ) {
    return "agents_turn";
  }
  if (live.some((e) => e.status === "pending")) return "waiting_for_review";
  // The rest are approved (≥1) and/or merged, no pending — approved, unless the
  // tip is still partial (the agent is pushing), which is agents_turn.
  return tip.partial ? "agents_turn" : "approved";
}

const newestEntryTime = (path: PathEntry[]): string => {
  // The newest member-entry time across the path; fall back to the latest
  // revision's created_at via the change set.
  let newest = "";
  for (const e of path) {
    const c = changes.find((x) => x.id === e.change_id);
    const rev = c?.revisions.find((r) => r.number === e.revision);
    for (const t of [
      rev?.created_at,
      ...threads
        .filter((th) => th.change_id === e.change_id)
        .map((th) => th.updated_at),
    ]) {
      if (t && t > newest) newest = t;
    }
  }
  return newest;
};

function chainSummary(tip: TipRecord): ChainSummary {
  const path = derivePath(tip);
  return {
    tip_change_id: tip.tip_change_id,
    repo_id: tip.repo_id,
    name: tip.name,
    state: chainState(tip, path),
    partial: tip.partial,
    updated_at: newestEntryTime(path),
    path,
  };
}

function chainView(tip: TipRecord): Chain {
  const path = derivePath(tip);
  const repo = repos.find((r) => r.id === tip.repo_id);
  return {
    tip_change_id: tip.tip_change_id,
    repo_id: tip.repo_id,
    name: tip.name,
    base_branch: repo?.base_branch ?? "main",
    state: chainState(tip, path),
    partial: tip.partial,
    path,
  };
}

/** Resolve `GET /chains/{change_id}?revision=N` to a tip (mirrors the backend's
 * `tip_for`): a live tip whose path walks `changeId` at that revision, else the
 * change as its own degenerate tip. So an INTERIOR change resolves to the tip
 * that extends through it (the full chain), not a 404. */
function resolveTip(
  changeId: number,
  revision?: number,
): TipRecord | undefined {
  const c = changes.find((x) => x.id === changeId);
  if (!c) return undefined;
  const rev = revision ?? latestRevision(c).number;
  for (const tip of tips) {
    const member = derivePath(tip).find((e) => e.change_id === changeId);
    if (member?.revision === rev) return tip;
  }
  // No live tip pins this (change, revision): the change is its own tip.
  return {
    tip_change_id: changeId,
    repo_id: c.repo_id,
    revision: rev,
    name: c.change_key.slice(0, 8),
    partial: false,
    active: !c.terminal,
  };
}

/** Every tip walking through a change, each with the patchset it pins there
 * (docs/api.md `ChainRef`). */
function chainsThrough(c: ChangeRecord): ChainRef[] {
  const refs: ChainRef[] = [];
  for (const tip of tips) {
    const member = derivePath(tip).find((e) => e.change_id === c.id);
    if (!member) continue;
    refs.push({
      tip_change_id: tip.tip_change_id,
      revision: member.revision,
      name: tip.name,
    });
  }
  return refs;
}

/** Derive the repo registry (docs/api.md `GET /api/repos`). `active_chains`
 * is the live tip count for the repo. */
function repoList(): Repo[] {
  return repos.map((r) => ({
    id: r.id,
    git_dir: r.git_dir,
    base_branch: r.base_branch,
    active_chains: tips.filter((t) => t.repo_id === r.id && t.active).length,
  }));
}

// ---------------------------------------------------------------------------
// Graph (docs/api.md "Graph"). The open region is the real chain derivation
// (active tips, unioned and deduped by sha); the canonical history below HEAD
// is synthetic (see ./data). Includes a merge commit and (per repo) a
// behind-HEAD base.

/** Topological row order, children before parents — mirrors the backend's
 * chain::graph_row_order (rank = longest path to a leaf, ties by input). */
function graphRowOrder(nodes: GraphNode[]): GraphNode[] {
  const index = new Map<string, number>();
  nodes.forEach((nd, i) => index.set(nd.commit_sha, i));
  const children: number[][] = nodes.map(() => []);
  nodes.forEach((nd, i) => {
    for (const p of nd.parents) {
      const pi = index.get(p);
      if (pi !== undefined) children[pi]?.push(i);
    }
  });
  const memo = new Array<number | undefined>(nodes.length).fill(undefined);
  const onStack = new Array<boolean>(nodes.length).fill(false);
  const rank = (i: number): number => {
    const cached = memo[i];
    if (cached !== undefined) return cached;
    if (onStack[i]) return 0;
    onStack[i] = true;
    const kids = children[i] ?? [];
    const r = kids.length > 0 ? 1 + Math.max(...kids.map(rank)) : 0;
    onStack[i] = false;
    memo[i] = r;
    return r;
  };
  const ranks = nodes.map((_, i) => rank(i));
  return nodes
    .map((nd, i) => ({ nd, i }))
    .sort((a, b) => (ranks[a.i] ?? 0) - (ranks[b.i] ?? 0) || a.i - b.i)
    .map((x) => x.nd);
}

/** The fixed merged-history window (mirrors the backend's MERGED_WINDOW). */
const MERGED_WINDOW = 5;

function buildGraph(repoId: number, window: number): RepoGraph {
  const repo = repos.find((r) => r.id === repoId) ?? notFound(`repo ${repoId}`);
  const scenario = graphScenarios[repoId] ?? { history: graphHistory };
  const fullHistory = scenario.history;
  const history = fullHistory.slice(0, window + 1);
  const historyTruncated = fullHistory.length > window + 1;
  const anchorSha = history[0]?.sha ?? "";

  const nodes: GraphNode[] = history.map((h, depth) => ({
    commit_sha: h.sha,
    section: depth === 0 ? "head" : "history",
    subject: h.subject,
    status: "merged",
    parents: h.parents,
    change_id: null,
    change_key: null,
    revision: null,
    counts: { threads: 0, drafts: 0, unresolved: 0 },
    draft_decision: null,
  }));
  const present = new Set(nodes.map((nd) => nd.commit_sha));

  // Walk tips in tip-sha order, mirroring the backend's leaves_where sort
  // (chain.rs) so the open-node input order — and thus the lane assignment —
  // matches production, not just the fixture declaration order.
  const activeTips = tips
    .filter((t) => t.repo_id === repoId && t.active)
    .slice()
    .sort((a, b) => {
      const sa = changes.find((c) => c.id === a.tip_change_id);
      const sb = changes.find((c) => c.id === b.tip_change_id);
      const ka = sa ? latestRevision(sa).commit_sha : "";
      const kb = sb ? latestRevision(sb).commit_sha : "";
      return ka < kb ? -1 : ka > kb ? 1 : 0;
    });
  const seen = new Set<string>();
  for (const tip of activeTips) {
    for (const m of walkPath(tip)) {
      const csha = m.revision.commit_sha;
      if (seen.has(csha)) continue;
      seen.add(csha);
      const { change: c, revision: rev } = m;
      const ownThreads = threads.filter(
        (t) => t.change_id === c.id && t.revision === rev.number,
      );
      const ownDrafts = drafts.filter(
        (d) => d.change_id === c.id && d.revision === rev.number,
      );
      nodes.push({
        commit_sha: csha,
        section: "open",
        subject: c.subject,
        status: statusAt(c, rev.number),
        parents: [rev.parent_sha],
        change_id: c.id,
        change_key: c.change_key,
        revision: rev.number,
        counts: {
          threads: ownThreads.length,
          drafts: ownDrafts.length,
          unresolved: ownThreads.filter((t) => !t.resolved).length,
        },
        draft_decision: draftReviews.get(c.id)?.decision ?? null,
      });
      present.add(csha);
    }
  }

  // An open chain's root parents onto the anchor unless its real base is in
  // the graph (e.g. a behind-HEAD fork onto a merged commit, below).
  for (const nd of nodes) {
    if (nd.section === "open" && !nd.parents.some((p) => present.has(p))) {
      nd.parents = [anchorSha];
    }
  }
  const behind = scenario.behind;
  if (behind) {
    const target = nodes.find(
      (nd) => nd.section === "open" && nd.change_id === behind.change_id,
    );
    // Reference the FULL history: a depth beyond the window slice is a base
    // older than the window, which dangles into the collapsed marker.
    const base = fullHistory[behind.depth];
    if (target && base) target.parents = [base.sha];
  }

  // Row order (mirrors build_graph): the open region ascends above HEAD, so
  // order it topologically among itself; the HEAD anchor + history keep the
  // canonical-walk order below it.
  const openNodes = nodes.filter((nd) => nd.section === "open");
  const rest = nodes.filter((nd) => nd.section !== "open");
  const openShas = new Set(openNodes.map((nd) => nd.commit_sha));
  const openOrder = graphRowOrder(
    openNodes.map((nd) => ({
      ...nd,
      parents: nd.parents.filter((p) => openShas.has(p)),
    })),
  );
  const orderIndex = new Map(openOrder.map((nd, i) => [nd.commit_sha, i]));
  openNodes.sort(
    (a, b) =>
      (orderIndex.get(a.commit_sha) ?? 0) - (orderIndex.get(b.commit_sha) ?? 0),
  );

  return {
    repo_id: repoId,
    base_branch: repo.base_branch,
    anchor: anchorSha,
    history_truncated: historyTruncated,
    nodes: [...openNodes, ...rest],
  };
}

/** A thread/draft record → its wire shape; anchors are served verbatim (the
 * client places them by diff range, docs/api.md "Comment placement"). */
function renderThread(t: ThreadRecord): Thread {
  return { ...t, range: t.range ?? null };
}
function renderDraft(d: DraftRecord): Draft {
  return { ...d, range: d.range ?? null };
}

function changeDetail(c: ChangeRecord): ChangeDetail {
  return {
    id: c.id,
    repo_id: c.repo_id,
    change_key: c.change_key,
    subject: c.subject,
    revisions: c.revisions,
    threads: threads.filter((x) => x.change_id === c.id).map(renderThread),
    drafts: drafts.filter((x) => x.change_id === c.id).map(renderDraft),
    reviews: c.reviews,
    chains: chainsThrough(c),
    draft_decision: draftReviews.get(c.id) ?? null,
  };
}

/** Find the text of a diff line so new drafts get a line_text snapshot. */
function snapshotLineText(
  c: ChangeRecord,
  revision: number,
  file: string | undefined,
  line: number | undefined,
  side: CommentSide,
): string | null {
  if (!file || line === undefined) return null;
  const diff = c.diffs[diffKey(revision)];
  const f = diff?.files.find((x) => x.path === file || x.old_path === file);
  if (!f) return null;
  for (const hunk of f.hunks) {
    for (const l of hunk.lines) {
      if (side === "new" ? l.new === line : l.old === line) return l.text;
    }
  }
  return null;
}

const notFound = (what: string): never => {
  throw new ApiError(404, `${what} not found`);
};

const getChange = (id: number): ChangeRecord =>
  changes.find((c) => c.id === id) ?? notFound(`change ${id}`);

// ---------------------------------------------------------------------------
// The mock router — mirrors the endpoint table in docs/api.md

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

  if ((m = /^\/repos\/(\d+)\/graph$/.exec(p)) && method === "GET") {
    const id = Number(m[1]);
    if (!repos.some((r) => r.id === id)) return notFound(`repo ${id}`);
    return buildGraph(id, MERGED_WINDOW);
  }

  if (method === "GET" && p === "/chains") {
    const status = q.get("status") ?? "active";
    const repo = q.get("repo");
    const listed = tips.filter(
      (t) =>
        (status === "all" || t.active) &&
        (repo === null || t.repo_id === Number(repo)),
    );
    return { chains: listed.map(chainSummary) };
  }

  // The aggregated chain log is not in this cut (events return later); serve
  // an empty timeline so the endpoint exists.
  if ((m = /^\/chains\/(\d+)\/log$/.exec(p)) && method === "GET") {
    const id = Number(m[1]);
    if (!tips.some((t) => t.tip_change_id === id))
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

  // Batch submit: publish every chain member's staged decision at the revision
  // the path pins, each independently (docs/api.md "Chains").
  if ((m = /^\/chains\/(\d+)\/submit$/.exec(p)) && method === "POST") {
    const id = Number(m[1]);
    const revision = q.has("revision") ? Number(q.get("revision")) : undefined;
    const tip = resolveTip(id, revision);
    if (!tip) return notFound(`chain ${id}`);
    const now = new Date().toISOString();
    let submitted = 0;
    const errors: { change_id: number; message: string }[] = [];
    for (const member of derivePath(tip)) {
      const staged = draftReviews.get(member.change_id);
      if (!staged) continue; // no decision — leave the member's comment drafts
      const c = changes.find((x) => x.id === member.change_id);
      if (!c) continue;
      const block = decisionBlock(c, staged.decision);
      if (block) {
        errors.push({ change_id: c.id, message: block });
        continue;
      }
      publishMember(c, staged.decision, staged.message, member.revision, now);
      draftReviews.delete(c.id);
      submitted++;
    }
    return { submitted, errors };
  }

  if ((m = /^\/changes\/(\d+)$/.exec(p)) && method === "GET") {
    return changeDetail(getChange(Number(m[1])));
  }

  if (
    (m = /^\/changes\/(\d+)\/revisions\/(\d+)\/diff$/.exec(p)) &&
    method === "GET"
  ) {
    const c = getChange(Number(m[1]));
    const revision = Number(m[2]);
    const against = q.has("against") ? Number(q.get("against")) : undefined;
    const rev = c.revisions.find((r) => r.number === revision);
    if (!rev) notFound(`revision ${revision}`);
    const diff = c.diffs[diffKey(revision, against)];
    if (!diff) notFound(`diff for revision ${revision}`);
    return structuredClone(diff);
  }

  if ((m = /^\/changes\/(\d+)\/drafts$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    const req = body as CreateDraftRequest;
    const side: CommentSide = req.side ?? "new";
    const now = new Date().toISOString();
    const record: DraftRecord = {
      id: nextDraftId++,
      change_id: c.id,
      thread_id: req.thread_id ?? null,
      revision: req.revision,
      file: req.file ?? null,
      line: req.line ?? null,
      side,
      range: req.range ?? null,
      line_text: snapshotLineText(c, req.revision, req.file, req.line, side),
      body: req.body,
      resolved: req.resolved ?? false,
      created_at: now,
      updated_at: now,
    };
    drafts.push(record);
    return renderDraft(record);
  }

  if ((m = /^\/drafts\/(\d+)$/.exec(p)) && method === "PATCH") {
    const id = Number(m[1]);
    const d = drafts.find((x) => x.id === id);
    if (!d) return notFound(`draft ${id}`);
    const req = body as { body: string; resolved?: boolean };
    d.body = req.body;
    if (req.resolved !== undefined) d.resolved = req.resolved;
    d.updated_at = new Date().toISOString();
    return renderDraft(d);
  }

  if ((m = /^\/drafts\/(\d+)$/.exec(p)) && method === "DELETE") {
    const id = Number(m[1]);
    const i = drafts.findIndex((x) => x.id === id);
    if (i < 0) notFound(`draft ${id}`);
    drafts.splice(i, 1);
    return undefined;
  }

  // Stage / clear a reviewer decision (drafted like a comment; published by the
  // chain batch submit above) — docs/api.md "Reviewer decisions".
  if ((m = /^\/changes\/(\d+)\/decision$/.exec(p)) && method === "PUT") {
    const c = getChange(Number(m[1]));
    const req = body as StageDecisionRequest;
    const staged = { decision: req.decision, message: req.message };
    draftReviews.set(c.id, staged);
    return staged;
  }

  if ((m = /^\/changes\/(\d+)\/decision$/.exec(p)) && method === "DELETE") {
    const c = getChange(Number(m[1]));
    draftReviews.delete(c.id);
    return undefined;
  }

  if ((m = /^\/changes\/(\d+)\/abandon$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    c.terminal ??= "abandoned";
    return changeDetail(c);
  }

  if ((m = /^\/changes\/(\d+)\/reopen$/.exec(p)) && method === "POST") {
    const c = getChange(Number(m[1]));
    if (c.terminal === "abandoned") c.terminal = undefined;
    return changeDetail(c);
  }

  throw new ApiError(404, `mock: no route for ${method} ${path}`);
}
