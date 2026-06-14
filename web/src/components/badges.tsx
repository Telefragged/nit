import type { ChainState, ChangeStatus } from "../api/types";

// Color discipline (docs/frontend.md): amber = needs reviewer, blue = agent
// working, green = approved/ready, red = changes requested, gray = inert.

const STATE_LABEL: Record<ChainState, string> = {
  waiting_for_review: "WAITING FOR REVIEW",
  agents_turn: "AGENT'S TURN",
  approved: "APPROVED",
  merged: "MERGED",
  abandoned: "ABANDONED",
};

const STATE_COLOR: Record<ChainState, string> = {
  waiting_for_review: "amber",
  agents_turn: "blue",
  approved: "green",
  merged: "gray",
  abandoned: "gray",
};

export function StateBadge({ state }: { state: ChainState }) {
  return (
    <span className={`badge badge-${STATE_COLOR[state]}`}>
      {STATE_LABEL[state]}
    </span>
  );
}

/**
 * Sticky partial-chain marker: the agent is still pushing commits.
 * Informational, not a call to action — gray, never amber.
 */
export function PartialBadge() {
  return <span className="badge badge-gray">PARTIAL</span>;
}

const STATUS_LABEL: Record<ChangeStatus, string> = {
  pending: "PENDING",
  approved: "APPROVED",
  changes_requested: "CHANGES REQUESTED",
  commented: "COMMENTED",
  orphaned: "ORPHANED",
};

const STATUS_COLOR: Record<ChangeStatus, string> = {
  pending: "amber",
  approved: "green",
  changes_requested: "red",
  commented: "blue",
  orphaned: "gray",
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
