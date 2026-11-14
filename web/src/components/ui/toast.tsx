import { Toast as ToastPrimitive } from "@base-ui/react/toast";
import { X } from "lucide-react";
import { Button } from "./button";

export const ToastProvider = ToastPrimitive.Provider;

/** The toasts of the enclosing `ToastProvider`, stacked in the bottom right
 * corner of the viewport. Each toast has a close button, and the button of
 * its `actionProps` when it has them. */
export function Toaster() {
  const { toasts } = ToastPrimitive.useToastManager();
  return (
    <ToastPrimitive.Portal>
      <ToastPrimitive.Viewport className="fixed right-4 bottom-4 z-50 flex w-80 flex-col gap-2 outline-none">
        {toasts.map((toast) => (
          <ToastPrimitive.Root
            key={toast.id}
            toast={toast}
            data-slot="toast"
            className="flex items-start gap-2 rounded-lg border border-input bg-card py-2.5 pr-2 pl-3 text-sm shadow-overlay outline-none"
          >
            <ToastPrimitive.Content className="flex min-w-0 flex-1 flex-col gap-0.5 pt-0.5">
              <ToastPrimitive.Title className="font-semibold" />
              <ToastPrimitive.Description className="text-muted-foreground" />
            </ToastPrimitive.Content>
            <ToastPrimitive.Action render={<Button size="sm" />} />
            <ToastPrimitive.Close
              aria-label="Close"
              render={<Button variant="ghost" size="icon" />}
            >
              <X size={14} aria-hidden />
            </ToastPrimitive.Close>
          </ToastPrimitive.Root>
        ))}
      </ToastPrimitive.Viewport>
    </ToastPrimitive.Portal>
  );
}
