import type { ChangeStatus } from "../api/types";

// Color discipline (docs/frontend.md): amber = needs reviewer, blue = agent
// working, green = approved/ready, red = changes requested, gray = inert.

/**
 * A path member whose change has a newer patchset on another chain (the path
 * pins an older revision). Informational — gray.
 */
export function NewerElsewhereBadge({
  revision,
  latest,
}: {
  revision: number;
  latest: number;
}) {
  return (
    <span
      className="badge badge-gray"
      title={`A newer revision (r${latest}) of this change lives on another chain; this chain pins r${revision}`}
    >
      NEWER ELSEWHERE
    </span>
  );
}

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
  commented: "blue",
  merged: "gray",
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
