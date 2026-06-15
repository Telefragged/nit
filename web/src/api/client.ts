// The ONLY place fetch happens. Components go through these functions (via
// react-query); when VITE_MOCK is set every call is answered by the
// contract-true fixtures in fixtures.ts instead of the network.

import type {
  Chain,
  ChainList,
  ChangeDetail,
  Comment,
  CreateDraftRequest,
  Diff,
  Health,
  RepoList,
  SubmitReviewRequest,
  SubmitReviewResponse,
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

type Method = "GET" | "POST" | "PATCH" | "DELETE";

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
// Health

export const getHealth = () => request<Health>("GET", "/health");

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

export const getChain = (id: number) => request<Chain>("GET", `/chains/${id}`);

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
  request<Comment>("POST", `/changes/${changeId}/drafts`, draft);

export const updateDraft = (id: number, req: UpdateDraftRequest) =>
  request<Comment>("PATCH", `/drafts/${id}`, req);

export const deleteDraft = (id: number) => request("DELETE", `/drafts/${id}`);

// ---------------------------------------------------------------------------
// Reviews

export const submitReview = (changeId: number, review: SubmitReviewRequest) =>
  request<SubmitReviewResponse>("POST", `/changes/${changeId}/reviews`, review);
