import type { ChangeStatus, Decision, Verdict } from "./types";

/** The displayed status a bare verdict maps to (mirrors the server's
 * `From<Verdict> for ChangeStatus`). */
export const verdictStatus: Record<Verdict, ChangeStatus> = {
  approve: "approved",
  request_changes: "changes_requested",
  comment: "commented",
};

/** The name of a decision, as the reviewer reads it. */
export const DECISION_LABEL: Record<Decision, string> = {
  approve: "Approve",
  request_changes: "Request changes",
  comment: "Comment",
  abandon: "Abandon",
  reopen: "Reopen",
};
