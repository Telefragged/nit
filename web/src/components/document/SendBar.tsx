import { useShortcut } from "../../lib/shortcutKey";
import { Button } from "../ui/button";
import { Kbd } from "../ui/kbd";

/** The bar at the bottom of the document page. Its button sends the
 * version on screen to the author, as Submit of the review page publishes
 * the reviews. `sent` is whether that version is the last one sent. `failure`
 * is why the last edit or send did not reach the server, or null. */
export default function SendBar({
  sent,
  sending,
  failure,
  onSend,
}: {
  sent: boolean;
  sending: boolean;
  failure: string | null;
  onSend: () => void;
}) {
  const canSend = !sent && !sending;
  useShortcut("s", canSend, onSend);
  return (
    <div className="review-bar">
      <span className="counts review-stats">
        {failure !== null ? (
          <span className="text-del" role="alert">
            {failure}
          </span>
        ) : null}
      </span>
      <div className="review-bar-actions">
        <Button
          variant="primary"
          disabled={!canSend}
          aria-keyshortcuts="s"
          title={
            sent
              ? "No edits since the last send"
              : "Send the edits of this document to the author"
          }
          onClick={onSend}
        >
          Send to author <Kbd aria-hidden>S</Kbd>
        </Button>
      </div>
    </div>
  );
}
