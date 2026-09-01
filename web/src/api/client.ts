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
  DiffFile,
  DiffMode,
  Draft,
  FileLines,
  Repo,
  RepoHistory,
  RepoList,
  DraftDecision,
  EditDraft,
  TagList,
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
export const getChain = (changeNumber: number, revision?: number) =>
  request<Chain>(
    "GET",
    revision === undefined
      ? `/chains/${changeNumber}`
      : `/chains/${changeNumber}?revision=${revision}`,
  );

const statusQuery = (statuses: ChangeStatus[]) =>
  statuses.map((s) => `&status=${s}`).join("");

/** A repo's changes as folded projections, narrowed to the statuses named —
 * the filter is explicit and repeatable; the API serves no default subset. */
export const getChanges = (repoId: number, statuses: ChangeStatus[]) =>
  request<ChangeList>("GET", `/changes?repo=${repoId}${statusQuery(statuses)}`);

/** The tags the repo's changes at `statuses` carry now, grouped by key. */
export const getTags = (repoId: number, statuses: ChangeStatus[]) =>
  request<TagList>("GET", `/tags?repo=${repoId}${statusQuery(statuses)}`);

/** A window of the repo's canonical ref below its HEAD; the window is
 * fixed server-side. */
export const getHistory = (repoId: number) =>
  request<RepoHistory>("GET", `/history?repo=${repoId}`);

/** The reviewer's private overlay alone (drafts + draft decision); the change
 * page reads the published projection over the websocket instead. */
export const getChangeDrafts = (id: number) =>
  request<ChangeDrafts>("GET", `/changes/${id}/drafts`);

export const getDiff = (
  changeNumber: number,
  revision: number,
  against?: number,
  mode: DiffMode = "full",
) => {
  const q = new URLSearchParams();
  if (against !== undefined) q.set("against", String(against));
  if (mode !== "full") q.set("mode", mode);
  const query = q.size > 0 ? `?${q}` : "";
  return request<Diff>(
    "GET",
    `/changes/${changeNumber}/revisions/${revision}/diff${query}`,
  );
};

/** The whole file as diff lines, over the same trees as `getDiff`
 * (`against` selects the interdiff base), for revealing the runs the shown
 * hunks hide — drift and all.
 *
 * Pass the file itself, not just its path: the server bounds its tree diffs
 * to both names, and only a bound holding both ends of a rename pairs it. */
export const getFileLines = (
  changeNumber: number,
  revision: number,
  file: Pick<DiffFile, "path" | "old_path">,
  against?: number,
) => {
  const q = new URLSearchParams({ path: file.path });
  if (file.old_path !== undefined) q.set("old_path", file.old_path);
  if (against !== undefined) q.set("against", String(against));
  return request<FileLines>(
    "GET",
    `/changes/${changeNumber}/revisions/${revision}/lines?${q}`,
  );
};

export const createDraft = (changeNumber: number, draft: NewDraft) =>
  request<Draft>("POST", `/changes/${changeNumber}/drafts`, draft);

export const updateDraft = (id: number, req: EditDraft) =>
  request<Draft>("PATCH", `/drafts/${id}`, req);

export const deleteDraft = (id: number) => request("DELETE", `/drafts/${id}`);

// Reviewer decisions (drafted like comments, published per chain)

/** Set (or overwrite) a change's draft decision. */
export const setDraftDecision = (changeNumber: number, req: DraftDecision) =>
  request<DraftDecision>("PUT", `/changes/${changeNumber}/decision`, req);

export const clearDecision = (changeNumber: number) =>
  request("DELETE", `/changes/${changeNumber}/decision`);

/** Publish every member's draft decision for the chain rooted at `tipChangeId`.
 * `revision` picks the chain context (the tip's own), like getChain. */
export const submitChain = (tipChangeId: number, revision?: number) =>
  request<BatchSubmitResult>(
    "POST",
    revision === undefined
      ? `/chains/${tipChangeId}/submit`
      : `/chains/${tipChangeId}/submit?revision=${revision}`,
  );
