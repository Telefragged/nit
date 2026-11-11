import Modal from "./Modal";
import { Button } from "./ui/button";

const nothing = () => undefined;

/** A modal with each of its parts. The dialog is in the top layer and
 * centers itself in the viewport, so the story is as large as the
 * viewport. */
export function Default() {
  return (
    <div className="h-screen w-screen">
      <Modal
        title="Review settings"
        aside={<span className="dim">Saved in this browser</span>}
        card="settings-modal"
        onDismiss={nothing}
        actions={
          <>
            <Button variant="ghost">Cancel</Button>
            <span className="spacer" />
            <Button>Apply</Button>
          </>
        }
      >
        The body of a modal holds its fields.
      </Modal>
    </div>
  );
}
