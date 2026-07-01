import type { ChangeStatus, Verdict } from "./types";

/** The displayed status a bare verdict maps to (mirrors the server's
 * `From<Verdict> for ChangeStatus`). Terminal statuses come from lifecycle,
 * not a verdict, so they are not here. */
export const verdictStatus: Record<Verdict, ChangeStatus> = {
  approve: "approved",
  request_changes: "changes_requested",
  comment: "commented",
};
