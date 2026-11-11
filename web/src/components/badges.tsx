import type { ChangeStatus } from "../api/types";

const STATUS_LABEL: Record<ChangeStatus, string> = {
  pending: "Pending",
  approved: "Approved",
  changes_requested: "Changes requested",
  commented: "Commented",
  merged: "Merged",
  abandoned: "Abandoned",
};

// Amber: the next action is the reviewer's. Green, red and purple: the
// three outcomes. Hollow: a review without a verdict.
const STATUS_COLOR: Record<ChangeStatus, string> = {
  pending: "amber",
  approved: "green",
  changes_requested: "red",
  commented: "hollow",
  merged: "purple",
  abandoned: "gray",
};

/** A change status as its dot and its name. */
export function StatusChip({ status }: { status: ChangeStatus }) {
  return (
    <span className="status">
      <StatusDot status={status} />
      {STATUS_LABEL[status]}
    </span>
  );
}

export function StatusDot({
  status,
  title,
}: {
  status: ChangeStatus;
  title?: string;
}) {
  return <span className={`dot dot-${STATUS_COLOR[status]}`} title={title} />;
}
