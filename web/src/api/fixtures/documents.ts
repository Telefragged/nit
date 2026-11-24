import { DocumentReplica } from "../../lib/wasm";
import { ApiError } from "../client";
import type { DocumentSummary, Version } from "../document.gen";
import { ATTACHMENT_PREFIX } from "../types";
import blobs from "./blobs.json";

interface DocumentRecord {
  number: number;
  title: string;
  sent: Version | null;
  replica: DocumentReplica;
}

const file = (revision: number, path: string, rest = "") =>
  `/changes/11?revision=${revision}${rest}&file=${encodeURIComponent(path)}`;
const ROTATE = "src/auth/rotate.rs";
const attachment = (name: string) =>
  `/changes/12?revision=0&file=${encodeURIComponent(ATTACHMENT_PREFIX + name)}`;

const ROTATION_PLAN = `Three changes add rotation of refresh tokens. Read them in this order: the schema, the rotation, and then the documentation.

[change 10](/changes/10)

[change 11](/changes/11)

## Reuse revokes the family

\`rotate\` looks up the presented token. A token that was rotated before is a sign of theft, so the whole family is revoked.

[rotate.rs](${file(1, ROTATE)}&line=20-28)

Revision 0 trusted the lookup. Revision 1 rewrote these lines, and [the test](${file(1, "tests/rotation.rs")}) covers the reuse.

[rotate.rs at revision 0](${file(0, ROTATE)}&line=20-21)

## The store after the review

The interdiff from revision 0 to revision 1 shows what the review changed:

- The lookup trims the presented token.
- A rebase renamed the parameter of \`insert\`.

[store.rs](${file(1, "src/auth/store.rs", "&against=r0")})

## The flow and the plan

[flow.svg](${attachment("web/screenshots/flow.svg")})

[rotation.md](${attachment("plan/rotation.md")})

[nit-log.txt](/api/attachments/${"a".repeat(64)}/nit-log.txt)
`;

/** The snapshot of a document with no text, in hexadecimal. */
const EMPTY =
  "6c6f726f000000000000000000000000532e0cfc00032f0000004c4f524f0000000200767600000001000200e762c96c0100000005000000020066720002007676dbd8c9c8160000000000000000000000";

function rotationPlan(): DocumentRecord {
  const bytes = EMPTY.match(/../g) ?? [];
  const replica = new DocumentReplica(
    Uint8Array.from(bytes, (byte) => parseInt(byte, 16)),
  );
  replica.set_text(ROTATION_PLAN);
  return {
    number: 1,
    title: "Refresh token rotation",
    sent: null,
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
