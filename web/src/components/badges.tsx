import type { ChangeStatus } from "../api/types";

// Amber needs the reviewer. Green, red and purple are the three outcomes.

const STATUS_LABEL: Record<ChangeStatus, string> = {
  pending: "PENDING",
  approved: "APPROVED",
  changes_requested: "CHANGES REQUESTED",
  commented: "COMMENTED",
  merged: "MERGED",
  abandoned: "ABANDONED",
};

const STATUS_COLOR: Record<ChangeStatus, string> = {
  pending: "amber",
  approved: "green",
  changes_requested: "red",
  commented: "gray",
  merged: "purple",
  abandoned: "gray",
};

export function StatusChip({ status }: { status: ChangeStatus }) {
  return (
    <span className={`badge badge-${STATUS_COLOR[status]}`}>
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
