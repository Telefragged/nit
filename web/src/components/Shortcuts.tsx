import Modal from "./Modal";
import { Button } from "./ui/button";
import { Kbd } from "./ui/kbd";

const SHORTCUTS: { keys: string[]; does: string }[] = [
  { keys: ["[", "]"], does: "Go to the previous or the next file" },
  { keys: ["n", "shift+n"], does: "Go to the next or the previous change" },
  { keys: ["r"], does: "Show the latest revision" },
  {
    keys: ["c"],
    does: "Comment on the selected text, or else on the linked lines",
  },
  { keys: ["a"], does: "Review this change" },
  { keys: ["s"], does: "Submit the drafted reviews" },
  { keys: ["ctrl+enter"], does: "Save the draft in the editor" },
  { keys: ["esc"], does: "Close the editor or the dialog" },
  { keys: ["?"], does: "Show this list" },
];

/** The list of the page's keys, in a modal. */
export function ShortcutList({ onClose }: { onClose: () => void }) {
  return (
    <Modal
      title="Keyboard shortcuts"
      card="shortcuts-modal"
      onDismiss={onClose}
      actions={<Button onClick={onClose}>Close</Button>}
    >
      <dl className="shortcut-list">
        {SHORTCUTS.map(({ keys, does }) => (
          <div key={does}>
            <dt>
              {keys.map((key) => (
                <Kbd key={key}>{key}</Kbd>
              ))}
            </dt>
            <dd>{does}</dd>
          </div>
        ))}
      </dl>
    </Modal>
  );
}

/**
 * The diffbar's Shortcuts button, and the list while `open` is true. A
 * press on the button and the ways out of the list call `onOpenChange`.
 */
export default function Shortcuts({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <>
      <Button
        variant="ghost"
        aria-keyshortcuts="?"
        onClick={() => {
          onOpenChange(true);
        }}
      >
        Shortcuts <Kbd aria-hidden>?</Kbd>
      </Button>
      {open ? (
        <ShortcutList
          onClose={() => {
            onOpenChange(false);
          }}
        />
      ) : null}
    </>
  );
}
