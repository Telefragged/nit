import { Dialog as DialogPrimitive } from "@base-ui/react/dialog";
import { cn } from "@/lib/utils";

export const Dialog = DialogPrimitive.Root;

export const DialogTitle = DialogPrimitive.Title;

/** The card of a dialog, centered over a scrim that covers the page. */
export function DialogContent({
  className,
  ...props
}: DialogPrimitive.Popup.Props) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Backdrop
        data-slot="dialog-overlay"
        className="fixed inset-0 z-50 bg-scrim"
      />
      <DialogPrimitive.Popup
        data-slot="dialog-content"
        className={cn(
          "fixed top-1/2 left-1/2 z-50 flex max-w-[calc(100vw-48px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-lg border border-input bg-card shadow-overlay outline-none",
          className,
        )}
        {...props}
      />
    </DialogPrimitive.Portal>
  );
}
