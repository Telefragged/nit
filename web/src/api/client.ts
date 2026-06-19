// The ONLY place fetch happens. Components go through these functions (via
// react-query); when VITE_MOCK is set every call is answered by the
// contract-true fixtures in fixtures.ts instead of the network.

import type {
  BatchSubmitResult,
  Chain,
  ChainList,
  ChangeDetail,
  CreateDraftRequest,
  Diff,
  Draft,
  RepoList,
  StagedDecision,
  StageDecisionRequest,
  UpdateDraftRequest,
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
    // Loaded lazily so fixtures stay out of production bundles.
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

// ---------------------------------------------------------------------------
// Repos

export const listRepos = () => request<RepoList>("GET", "/repos");

// ---------------------------------------------------------------------------
// Chains

export const listChains = (
  status: "active" | "all" = "active",
  repo?: number,
) =>
  request<ChainList>(
    "GET",
    `/chains?status=${status}${repo === undefined ? "" : `&repo=${repo}`}`,
  );

/** The derived chain through a change's tip. `revision` selects which patchset
 * of the change to root on (and hence the chain context). */
export const getChain = (changeId: number, revision?: number) =>
  request<Chain>(
    "GET",
    revision === undefined
      ? `/chains/${changeId}`
      : `/chains/${changeId}?revision=${revision}`,
  );

// ---------------------------------------------------------------------------
// Changes

export const getChange = (id: number) =>
  request<ChangeDetail>("GET", `/changes/${id}`);

export const getDiff = (changeId: number, revision: number, against?: number) =>
  request<Diff>(
    "GET",
    against === undefined
      ? `/changes/${changeId}/revisions/${revision}/diff`
      : `/changes/${changeId}/revisions/${revision}/diff?against=${against}`,
  );

// ---------------------------------------------------------------------------
// Drafts

export const createDraft = (changeId: number, draft: CreateDraftRequest) =>
  request<Draft>("POST", `/changes/${changeId}/drafts`, draft);

export const updateDraft = (id: number, req: UpdateDraftRequest) =>
  request<Draft>("PATCH", `/drafts/${id}`, req);

export const deleteDraft = (id: number) => request("DELETE", `/drafts/${id}`);

// ---------------------------------------------------------------------------
// Reviewer decisions (staged like comment drafts, published per chain)

/** Stage (or overwrite) a change's draft decision — a verdict or an
 * abandon/reopen (docs/api.md "Reviewer decisions"). */
export const stageDecision = (changeId: number, req: StageDecisionRequest) =>
  request<StagedDecision>("PUT", `/changes/${changeId}/decision`, req);

/** Discard a change's staged decision. */
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
