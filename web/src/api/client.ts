// The ONLY place the nit API is fetched. Components go through these
// functions (via react-query); when VITE_MOCK is set every call is answered
// by the contract-true fixtures in ./fixtures instead of the network.

import type { DocumentNumber, DocumentSummary, Version } from "./document.gen";
import type {
  AttachmentHash,
  BatchSubmitResult,
  ChangeDrafts,
  ChangeList,
  ChangeQuery,
  ChangeStatus,
  Diff,
  DiffFile,
  Draft,
  DraftDecision,
  EditDraft,
  EffectiveAttachment,
  FileLines,
  Health,
  NewDraft,
  PortedComment,
  Repo,
  RepoHistory,
  RepoList,
  TagList,
  ChangeVersion,
  Whitespace,
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

/** The answer of the fixtures to a request. The import is dynamic, so
 * production bundles contain no fixtures. */
const mocked = async (method: Method, path: string, body?: unknown) =>
  (await import("./fixtures")).mockRequest(method, path, body);

const decode = {
  json: (res: Response): Promise<unknown> =>
    res.status === 204 ? Promise.resolve(undefined) : res.json(),
  bytes: async (res: Response) => new Uint8Array(await res.arrayBuffer()),
  text: (res: Response) =>
    /^text\/|json/.test(res.headers.get("Content-Type") ?? "")
      ? res.text()
      : null,
};

const isBytes = (body: unknown): body is Blob | Uint8Array<ArrayBuffer> =>
  body instanceof Blob || body instanceof Uint8Array;

async function request<T = void>(
  method: Method,
  path: string,
  body?: unknown,
  kind: keyof typeof decode = "json",
): Promise<T> {
  if (import.meta.env.VITE_MOCK) return (await mocked(method, path, body)) as T;
  const json = body !== undefined && !isBytes(body);
  const res = await fetch(`/api${path}`, {
    method,
    headers: json ? { "Content-Type": "application/json" } : undefined,
    body: isBytes(body) ? body : json ? JSON.stringify(body) : undefined,
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
  return (await decode[kind](res)) as T;
}

export const getHealth = () => request<Health>("GET", "/health");

export const listRepos = () => request<RepoList>("GET", "/repos");

export const getRepo = (id: number) => request<Repo>("GET", `/repos/${id}`);

const statusQuery = (statuses: ChangeStatus[]) =>
  statuses.map((s) => `&status=${s}`).join("");

/** A change query as its query string: each list field repeats its key. */
const changeQuery = (query: ChangeQuery) => {
  const params = new URLSearchParams();
  if (query.repo !== undefined) params.set("repo", String(query.repo));
  for (const status of query.status ?? []) params.append("status", status);
  for (const tag of query.tag ?? []) params.append("tag", tag);
  if (query.change_id !== undefined) params.set("change_id", query.change_id);
  return params.toString();
};

/** The changes `query` picks, as folded projections. */
export const getChanges = (query: ChangeQuery) =>
  request<ChangeList>("GET", `/changes?${changeQuery(query)}`);

/** The tags the repo's changes at `statuses` carry now, grouped by key. */
export const getTags = (repoId: number, statuses: ChangeStatus[]) =>
  request<TagList>("GET", `/tags?repo=${repoId}${statusQuery(statuses)}`);

/** A window of the repo's canonical ref below its HEAD; the window is
 * fixed server-side. */
export const getHistory = (repoId: number) =>
  request<RepoHistory>("GET", `/history?repo=${repoId}`);

/** The reviewer's private overlay: the drafts and the draft decision. */
export const getChangeDrafts = (id: number) =>
  request<ChangeDrafts>("GET", `/changes/${id}/drafts`);

export const getDiff = (
  changeNumber: number,
  revision: number,
  against: ChangeVersion,
  whitespace: Whitespace = "compare",
) => {
  const q = new URLSearchParams({ against });
  if (whitespace !== "compare") q.set("whitespace", whitespace);
  return request<Diff>(
    "GET",
    `/changes/${changeNumber}/revisions/${revision}/diff?${q}`,
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
  against: ChangeVersion,
  whitespace: Whitespace = "compare",
) => {
  const q = new URLSearchParams({ path: file.path, against });
  if (file.old_path !== undefined) q.set("old_path", file.old_path);
  if (whitespace !== "compare") q.set("whitespace", whitespace);
  return request<FileLines>(
    "GET",
    `/changes/${changeNumber}/revisions/${revision}/lines?${q}`,
  );
};

/** The threads of earlier revisions at their place in `revision`'s trees
 * and, over the same range as `getDiff`, in `against`'s: `revision`'s
 * entries first, resolved or not. */
export const getPorted = (
  changeNumber: number,
  revision: number,
  against: ChangeVersion,
) => {
  return request<PortedComment[]>(
    "GET",
    `/changes/${changeNumber}/revisions/${revision}/ported?against=${against}`,
  );
};

export const createDraft = (changeNumber: number, draft: NewDraft) =>
  request<Draft>("POST", `/changes/${changeNumber}/drafts`, draft);

export const updateDraft = (id: number, req: EditDraft) =>
  request<Draft>("PATCH", `/drafts/${id}`, req);

export const deleteDraft = (id: number) => request("DELETE", `/drafts/${id}`);

/** Set (or overwrite) a change's draft decision. */
export const setDraftDecision = (changeNumber: number, req: DraftDecision) =>
  request<DraftDecision>("PUT", `/changes/${changeNumber}/decision`, req);

export const clearDecision = (changeNumber: number) =>
  request("DELETE", `/changes/${changeNumber}/decision`);

/** Publish the draft decision of every change `query` picks, each at its
 * latest revision. */
export const submitDecisions = (query: ChangeQuery) =>
  request<BatchSubmitResult>("POST", `/submit?${changeQuery(query)}`);

const attachmentPath = ({
  hash,
  name,
}: Pick<EffectiveAttachment, "hash" | "name">) =>
  `/attachments/${hash}/${encodeURIComponent(name.split("/").at(-1) ?? "")}`;

/** The URL of an attachment's bytes. The server picks the content type
 * from the name's last segment. */
export const attachmentUrl = (
  attachment: Pick<EffectiveAttachment, "hash" | "name">,
) => `/api${attachmentPath(attachment)}`;

/** The text of an attachment, or null when its content type is not
 * text. */
export const getAttachmentText = (
  attachment: Pick<EffectiveAttachment, "hash" | "name">,
) =>
  request<string | null>("GET", attachmentPath(attachment), undefined, "text");

export const getDocument = (number: DocumentNumber) =>
  request<DocumentSummary>("GET", `/documents/${number}`);

/** The bytes that a `DocumentReplica` loads. */
export const getDocumentSnapshot = (number: DocumentNumber) =>
  request<Uint8Array>(
    "GET",
    `/documents/${number}/snapshot`,
    undefined,
    "bytes",
  );

/** Merges the edits of a replica into the stored document. `update` is
 * the bytes of `DocumentReplica.updates_since`. */
export const postDocumentUpdate = (
  number: DocumentNumber,
  update: Uint8Array,
) => request("POST", `/documents/${number}/updates`, update);

/** Sends `version` of a document to the author. */
export const sendDocument = (number: DocumentNumber, version: Version) =>
  request<DocumentSummary>("POST", `/documents/${number}/send`, version);

/** Stores a file, for an upload reference of a document. */
export const uploadAttachment = (file: Blob) =>
  request<{ hash: AttachmentHash }>("PUT", "/attachments", file);
