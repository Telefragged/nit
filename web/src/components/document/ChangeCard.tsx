import { Link } from "react-router-dom";
import type { Revision } from "../../api/types";
import { StatusChip } from "../badges";

/** A change as a card: its number, and the subject and the status of
 * `revision`, its latest one. The card links to the review page. The
 * revision is absent until the change loads. */
export default function ChangeCard({
  number,
  revision,
}: {
  number: number;
  revision: Revision | undefined;
}) {
  return (
    <Link className="change-card" to={`/changes/${number}`}>
      <span className="change-card-number">change {number}</span>
      <span className="change-card-subject">{revision?.subject}</span>
      {revision ? <StatusChip status={revision.status} /> : null}
    </Link>
  );
}
