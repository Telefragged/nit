import { useLayoutEffect, useRef, type ReactNode } from "react";

/**
 * A native modal dialog holding `children` in a centered card.
 *
 * `showModal()` puts the dialog in the top layer and makes the page behind
 * it inert. Opening is mounting, so the caller renders the modal only while
 * it is open. The dialog itself offers two ways out, and `onDismiss` names
 * which one the reviewer took.
 */
export default function Modal({
  label,
  card,
  onDismiss,
  children,
}: {
  /** Names the dialog for assistive technology. */
  label: string;
  /** The card's own class, which carries its width. */
  card: string;
  onDismiss: (via: "escape" | "backdrop") => void;
  children: ReactNode;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);

  // Layout effect so the dialog is visible the frame it mounts. It runs
  // before the caller's own, which is where a modal focuses its field.
  useLayoutEffect(() => {
    dialogRef.current?.showModal();
  }, []);

  return (
    // The dialog is its own full-bleed backdrop, so a backdrop press lands
    // on the dialog element itself. Escape arrives as `cancel` wherever
    // focus sits.
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- keyboard dismiss is onCancel (Escape)
    <dialog
      ref={dialogRef}
      className="modal-backdrop"
      aria-label={label}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onDismiss("backdrop");
      }}
      onCancel={(e) => {
        e.preventDefault();
        onDismiss("escape");
      }}
    >
      <div className={`modal-card ${card}`}>{children}</div>
    </dialog>
  );
}
