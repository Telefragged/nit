import type { ChangeStatus } from "../api/types";
import { StatusChip } from "./badges";

const STATUSES: ChangeStatus[] = [
  "pending",
  "approved",
  "changes_requested",
  "commented",
  "merged",
  "abandoned",
];

/** Every status of a change. */
export function Statuses() {
  return (
    <div style={{ display: "flex", gap: 20 }}>
      {STATUSES.map((status) => (
        <StatusChip key={status} status={status} />
      ))}
    </div>
  );
}
