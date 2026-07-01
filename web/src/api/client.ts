// The ONLY place fetch happens. Components go through these functions (via
// react-query); when VITE_MOCK is set every call is answered by the
// contract-true fixtures in fixtures.ts instead of the network.

import type {
  BatchSubmitResult,
  Chain,
  ChangeDetail,
  ChangeDrafts,
  NewDraft,
  Diff,
  Draft,
  FileLines,
  Repo,
  RepoGraph,
  RepoList,
  StagedDecision,
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

/** The derived chain through a change's tip. `revision` selects which patchset
 * of the change to root on (and hence the chain context). */
export const getChain = (changeId: number, revision?: number) =>
  request<Chain>(
    "GET",
    revision === undefined
      ? `/chains/${changeId}`
      : `/chains/${changeId}?revision=${revision}`,
  );

/** The repo's spine-centered change graph (docs/api.md "Graph"): the source
 * for the dashboard. The merged-history window is fixed server-side. */
export const getRepoGraph = (repoId: number) =>
  request<RepoGraph>("GET", `/repos/${repoId}/graph`);

export const getChange = (id: number) =>
  request<ChangeDetail>("GET", `/changes/${id}`);

/** The reviewer's private overlay alone (drafts + staged decision); the change
 * page reads the published projection over the websocket instead (docs/api.md
 * "Events"). */
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
 * the shown hunks hide — drift and all (docs/api.md "Expanding context"). */
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

// Reviewer decisions (staged like comment drafts, published per chain)

/** Stage (or overwrite) a change's draft decision — a verdict or an
 * abandon/reopen (docs/api.md "Reviewer decisions"). */
export const stageDecision = (changeId: number, req: StagedDecision) =>
  request<StagedDecision>("PUT", `/changes/${changeId}/decision`, req);

export const clearDecision = (changeId: number) =>
  request("DELETE", `/changes/${changeId}/decision`);

/** Publish every member's staged decision for the chain rooted at `tipChangeId`.
 * `revision` picks the chain context (the tip's patchset), like getChain. */
export const submitChain = (tipChangeId: number, revision?: number) =>
  request<BatchSubmitResult>(
    "POST",
    revision === undefined
      ? `/chains/${tipChangeId}/submit`
      : `/chains/${tipChangeId}/submit?revision=${revision}`,
  );
