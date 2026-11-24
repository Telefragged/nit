import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import {
  add,
  addedFile,
  ago,
  ctx,
  del,
  shaOf,
  threadComment,
} from "../../api/fixtures/builders";
import {
  ATTACHMENT_PREFIX,
  type ChangeDetail,
  type ChangeDrafts,
  type Diff,
} from "../../api/types";
import { referencesByLine } from "../../lib/reference";
import DocumentBlocks from "./DocumentBlocks";

const PATH = "src/auth/rotate.rs";
const HASH = "a".repeat(64);
const PLAN = "plan/rotation.md";
const PLAN_HASH = "b".repeat(64);

const CHANGE: ChangeDetail = {
  id: 11,
  repo_id: 1,
  change_id: "I3f2d8a91c0b7e514",
  revisions: [
    {
      number: 0,
      commit_sha: shaOf("c11r0"),
      parent_sha: shaOf("a0"),
      fork_sha: shaOf("a0"),
      message: "auth: rotate refresh tokens on use",
      subject: "auth: rotate refresh tokens on use",
      created_at: ago(120),
      status: "changes_requested",
      attachments: [{ name: PLAN, hash: PLAN_HASH, revision: 0 }],
      base_attachments: [],
    },
  ],
  threads: [
    {
      id: 71,
      anchor: { file: { version: "r0", path: PATH, anchor: { line: 5 } } },
      resolved: false,
      comments: [
        threadComment(
          40,
          "Return an error for an unknown token. Do not panic.",
        ),
      ],
      created_at: ago(40),
      updated_at: ago(40),
    },
    {
      id: 72,
      anchor: {
        file: {
          version: "r0",
          path: ATTACHMENT_PREFIX + PLAN,
          anchor: { line: 4 },
        },
      },
      resolved: false,
      comments: [threadComment(30, "Revoke the family in one transaction.")],
      created_at: ago(30),
      updated_at: ago(30),
    },
  ],
  drafts: [],
  draft_decision: null,
};

const DIFF: Diff = {
  files: [
    {
      path: PATH,
      status: "modified",
      binary: false,
      additions: 2,
      deletions: 1,
      old_total: 6,
      new_total: 7,
      hunks: [
        {
          old_start: 1,
          old_lines: 6,
          new_start: 1,
          new_lines: 7,
          header: "",
          lines: [
            ctx(1, 1, "impl TokenRotator {"),
            ctx(
              2,
              2,
              "    /// Exchange `presented` for a fresh refresh token.",
            ),
            ctx(3, 3, "    pub fn rotate(&self, presented: &str) -> Token {"),
            del(4, "        let entry = self.store.lookup(presented);"),
            add(4, "        let entry = self.store"),
            add(5, "            .lookup(presented).unwrap();"),
            ctx(5, 6, "        self.store.mark_rotated(entry.id)"),
            ctx(6, 7, "    }"),
          ],
        },
      ],
    },
    addedFile(
      ATTACHMENT_PREFIX + PLAN,
      "## Rotation plan\n\n1. Issue a successor token on every use.\n2. Revoke the family when a rotated token returns.\n",
    ),
  ],
};

const NO_DRAFTS: ChangeDrafts = { drafts: [], draft_decision: null };

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: Infinity } },
});
queryClient.setQueryData(["change", 11], CHANGE);
queryClient.setQueryData(["drafts", 11], NO_DRAFTS);
queryClient.setQueryData(["diff", 11, 0, "r0^", "compare", PLAN_HASH], DIFF);
queryClient.setQueryData(
  ["attachment-text", HASH, "nit-log.txt"],
  "change 12  position 0  revision 3f2a9c1\nchange 12  position 1  r0  attach plan/rotation.md\n",
);
queryClient.setQueryData(
  ["attachment-text", HASH, "plan.md"],
  "## Rotation plan\n\n1. Issue a successor token on every use.\n2. Revoke the family when a rotated token returns.\n",
);

const NARRATION =
  "## Reuse revokes the family\n\n`rotate` looks up the presented token. A token that was rotated before is a sign of theft, so the store revokes the **whole family**. The [review page](/changes/11?revision=0&file=src%2Fauth%2Frotate.rs) shows the file.\n\n- The lookup returns an error for an unknown token.\n- The caller receives the successor.\n";

const FILE = "/changes/11?revision=0&file=src%2Fauth%2Frotate.rs";

function Page({
  text,
  editing = null,
}: {
  text: string;
  editing?: number | null;
}): ReactNode {
  return (
    <MemoryRouter>
      <QueryClientProvider client={queryClient}>
        <div className="document-page" style={{ width: 1068 }}>
          <DocumentBlocks
            text={text}
            references={referencesByLine(text)}
            actions={{
              editing,
              onEdit: () => undefined,
              edit: () => undefined,
            }}
          />
        </div>
      </QueryClientProvider>
    </MemoryRouter>
  );
}

export const Narration = () => <Page text={NARRATION} />;

export const Editor = () => <Page text={NARRATION} editing={3} />;

export const Change = () => <Page text="[change 11](/changes/11)" />;

export const File = () => <Page text={`[rotate.rs](${FILE})`} />;

export const FileRange = () => <Page text={`[rotate.rs](${FILE}&line=3-5)`} />;

export const Attachment = () => (
  <Page text="[rotation.md](/changes/11?revision=0&file=%2Fattachments%2Fplan%2Frotation.md)" />
);

export const Uploads = () => (
  <Page
    text={`The log of the change and the plan:\n\n[nit-log.txt](/api/attachments/${HASH}/nit-log.txt)\n\n[plan.md](/api/attachments/${HASH}/plan.md)\n`}
  />
);
