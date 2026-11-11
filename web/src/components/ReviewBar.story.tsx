import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { changeId, draft } from "../api/fixtures/builders";
import type { ChangeDetail } from "../api/types";
import ReviewBar from "./ReviewBar";

const client = new QueryClient();

const change = (over: Partial<ChangeDetail> = {}): ChangeDetail => ({
  id: 11,
  repo_id: 1,
  change_id: changeId("tray"),
  revisions: [],
  threads: [],
  drafts: [],
  draft_decision: null,
  ...over,
});

/** The bar at the bottom of a frame. The bar has a fixed position, and
 * `contain: layout` makes the frame its containing block. */
function Frame({
  change,
  drafted,
  unresolved,
}: {
  change: ChangeDetail;
  drafted: number;
  unresolved: number;
}) {
  const [open, setOpen] = useState(false);
  return (
    <QueryClientProvider client={client}>
      <div className="h-11 w-[900px] contain-layout">
        <ReviewBar
          change={change}
          tag="session-id=tray"
          drafted={drafted}
          selectedRevision={1}
          unresolved={unresolved}
          replyOpen={open}
          onReplyOpenChange={setOpen}
        />
      </div>
    </QueryClientProvider>
  );
}

/** A change with no work of the reviewer: Submit is off. */
export function Empty() {
  return <Frame change={change()} drafted={0} unresolved={0} />;
}

/** Drafts, open threads and a draft decision: Submit publishes two
 * reviews. */
export function Drafted() {
  return (
    <Frame
      change={change({
        drafts: [draft({ id: 1 }), draft({ id: 2 }), draft({ id: 3 })],
        draft_decision: { decision: "request_changes", message: "" },
      })}
      drafted={2}
      unresolved={6}
    />
  );
}
