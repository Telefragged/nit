// The ONLY place fetch happens. Components go through these functions (via
// react-query); when VITE_MOCK is set every call is answered by the
// contract-true fixtures in ./fixtures instead of the network.

import type {
  BatchSubmitResult,
  Chain,
  ChangeDrafts,
  ChangeList,
  ChangeStatus,
  NewDraft,
  Diff,
  Draft,
  FileLines,
  Repo,
  RepoHistory,
  RepoList,
  DraftDecision,
  EditDraft,
} from "./types";

export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

async function request<T = void>(
  method: Method,
  path: string,
  body?: unknown,
): Promise<T> {
  if (import.meta.env.VITE_MOCK) {
    // Keeps fixtures out of production bundles.
    const { mockRequest } = await import("./fixtures");
    return mockRequest(method, path, body) as Promise<T>;
  }
  const res = await fetch(`/api${path}`, {
    method,
    headers:
      body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      const parsed = (await res.json()) as { error?: string };
      if (parsed.error) message = parsed.error;
    } catch {
      // non-JSON error body; keep the status line
    }
    throw new ApiError(res.status, message);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

export const listRepos = () => request<RepoList>("GET", "/repos");

export const getRepo = (id: number) => request<Repo>("GET", `/repos/${id}`);

/** The derived chain through a change's tip. `revision` selects which
 * version of the change to root on (and hence the chain context). */
export const getChain = (changeId: number, revision?: number) =>
  request<Chain>(
    "GET",
    revision === undefined
      ? `/chains/${changeId}`
      : `/chains/${changeId}?revision=${revision}`,
  );

/** A repo's changes as folded projections, narrowed to the statuses named —
 * the filter is explicit and repeatable; the API serves no default subset. */
export const getChanges = (repoId: number, statuses: ChangeStatus[]) =>
  request<ChangeList>(
    "GET",
    `/changes?repo=${repoId}${statuses.map((s) => `&status=${s}`).join("")}`,
  );

/** A window of the repo's canonical ref below its HEAD; the window is
 * fixed server-side. */
export const getHistory = (repoId: number) =>
  request<RepoHistory>("GET", `/history?repo=${repoId}`);

/** The reviewer's private overlay alone (drafts + draft decision); the change
 * page reads the published projection over the websocket instead. */
export const getChangeDrafts = (id: number) =>
  request<ChangeDrafts>("GET", `/changes/${id}/drafts`);

export const getDiff = (changeId: number, revision: number, against?: number) =>
  request<Diff>(
    "GET",
    against === undefined
      ? `/changes/${changeId}/revisions/${revision}/diff`
      : `/changes/${changeId}/revisions/${revision}/diff?against=${against}`,
  );

/** File `path`'s full-context diff lines over the same trees as `getDiff`
 * (`against` selects the interdiff base), for revealing the unchanged runs
 * the shown hunks hide — drift and all. */
export const getFileLines = (
  changeId: number,
  revision: number,
  path: string,
  against?: number,
) => {
  const q = `path=${encodeURIComponent(path)}`;
  return request<FileLines>(
    "GET",
    `/changes/${changeId}/revisions/${revision}/lines?${
      against === undefined ? q : `${q}&against=${against}`
    }`,
  );
};

export const createDraft = (changeId: number, draft: NewDraft) =>
  request<Draft>("POST", `/changes/${changeId}/drafts`, draft);

export const updateDraft = (id: number, req: EditDraft) =>
  request<Draft>("PATCH", `/drafts/${id}`, req);

export const deleteDraft = (id: number) => request("DELETE", `/drafts/${id}`);

// Reviewer decisions (drafted like comments, published per chain)

/** Set (or overwrite) a change's draft decision. */
export const setDraftDecision = (changeId: number, req: DraftDecision) =>
  request<DraftDecision>("PUT", `/changes/${changeId}/decision`, req);

export const clearDecision = (changeId: number) =>
  request("DELETE", `/changes/${changeId}/decision`);

/** Publish every member's draft decision for the chain rooted at `tipChangeId`.
 * `revision` picks the chain context (the tip's own), like getChain. */
export const submitChain = (tipChangeId: number, revision?: number) =>
  request<BatchSubmitResult>(
    "POST",
    revision === undefined
      ? `/chains/${tipChangeId}/submit`
      : `/chains/${tipChangeId}/submit?revision=${revision}`,
  );
