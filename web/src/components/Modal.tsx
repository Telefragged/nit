import type { ReactNode, RefObject } from "react";
import { Dialog, DialogContent, DialogTitle } from "./ui/dialog";

/**
 * A modal dialog: a centered card with `title` in its head, `children` in
 * its body, and `actions` in a row at its bottom.
 *
 * Opening is mounting, so the caller renders the modal only while it is
 * open. Escape and a press on the backdrop call `onDismiss`. The modal
 * stays open unless the caller then removes it.
 */
export default function Modal({
  title,
  aside,
  card,
  onDismiss,
  initialFocus,
  actions,
  children,
}: {
  /** The heading of the card, and the accessible name of the dialog. */
  title: string;
  /** Shows at the right of the head. */
  aside?: ReactNode;
  /** The card's own class, which carries its width. */
  card: string;
  onDismiss: () => void;
  /** The element that has the focus when the modal opens. Without it, the
   * first control of the card has the focus. */
  initialFocus?: RefObject<HTMLElement | null>;
  actions: ReactNode;
  children: ReactNode;
}) {
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onDismiss();
      }}
    >
      <DialogContent className={card} initialFocus={initialFocus}>
        <div className="modal-head">
          <DialogTitle>{title}</DialogTitle>
          {aside}
        </div>
        <div className="modal-body">{children}</div>
        <div className="modal-actions">{actions}</div>
      </DialogContent>
    </Dialog>
  );
}
