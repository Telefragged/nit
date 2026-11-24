import { DocumentReplica } from "../../lib/wasm";
import { ApiError } from "../client";
import type { DocumentSummary, Run, Version } from "../document.gen";
import blobs from "./blobs.json";
import rotation from "./rotationPlan.json";

interface DocumentRecord {
  number: number;
  title: string;
  sent: Version | null;
  replica: DocumentReplica;
}

function rotationPlan(): DocumentRecord {
  const bytes = rotation.snapshot.match(/../g) ?? [];
  const replica = new DocumentReplica(
    Uint8Array.from(bytes, (byte) => parseInt(byte, 16)),
  );
  return {
    number: 1,
    title: "Refresh token rotation",
    sent: (replica.runs() as Run[])[1]?.version ?? null,
    replica,
  };
}

let records: DocumentRecord[] | undefined;
const all = () => (records ??= [rotationPlan()]);

const find = (number: number) => {
  const record = all().find((d) => d.number === number);
  if (!record) throw new ApiError(404, `document ${number} not found`);
  return record;
};

const summary = (record: DocumentRecord): DocumentSummary => ({
  number: record.number,
  title: record.title,
  version: record.replica.version(),
  sent: record.sent,
});

interface Follower {
  number: number;
  version: Version;
  onUpdate: (update: Uint8Array) => void;
}
const followers = new Set<Follower>();

function publish(record: DocumentRecord) {
  const version = record.replica.version();
  for (const follower of followers) {
    if (follower.number !== record.number || follower.version === version)
      continue;
    const update = record.replica.updates_since(follower.version);
    follower.version = version;
    follower.onUpdate(update);
  }
}

const uploads = new Map<string, string>();

/** The answer to a document route or to an attachment route. */
export function mockDocumentRequest(
  method: string,
  path: string,
  body: unknown,
): unknown {
  let m: RegExpExecArray | null;
  if ((m = /^\/documents\/(\d+)$/.exec(path)) && method === "GET") {
    return summary(find(Number(m[1])));
  }
  if ((m = /^\/documents\/(\d+)\/snapshot$/.exec(path)) && method === "GET") {
    return find(Number(m[1])).replica.updates_since("-");
  }
  if ((m = /^\/documents\/(\d+)\/updates$/.exec(path)) && method === "POST") {
    const record = find(Number(m[1]));
    record.replica.merge(body as Uint8Array);
    publish(record);
    return undefined;
  }
  if ((m = /^\/documents\/(\d+)\/send$/.exec(path)) && method === "POST") {
    const record = find(Number(m[1]));
    record.sent = body as Version;
    return summary(record);
  }
  if (method === "PUT" && path === "/attachments") {
    const hash = String(uploads.size + 1).padStart(64, "0");
    return (body as Blob).text().then((text) => {
      uploads.set(hash, text);
      return { hash };
    });
  }
  if ((m = /^\/attachments\/([0-9a-f]{64})\//.exec(path)) && method === "GET") {
    const uploaded = uploads.get(m[1] ?? "");
    if (uploaded !== undefined) return uploaded;
    const blob = (
      blobs as Record<string, { contentType: string; body: string } | undefined>
    )[m[1] ?? ""];
    if (!blob) throw new ApiError(404, "no such attachment");
    return blob.contentType.startsWith("text/") ? blob.body : null;
  }
  throw new ApiError(404, `mock: no route for ${method} ${path}`);
}

/** The mock of `followDocument`: sends the edits that a replica at
 * `version` lacks, then the edits of each later write. */
export function mockFollowDocument(
  number: number,
  version: Version,
  onUpdate: (update: Uint8Array) => void,
): () => void {
  const follower = { number, version, onUpdate };
  followers.add(follower);
  const record = all().find((d) => d.number === number);
  if (record) publish(record);
  return () => {
    followers.delete(follower);
  };
}
