import { Popover as PopoverPrimitive } from "@base-ui/react/popover";
import { cn } from "@/lib/utils";

export const Popover = PopoverPrimitive.Root;

export const PopoverTrigger = PopoverPrimitive.Trigger;

export const PopoverTitle = PopoverPrimitive.Title;

/** The card of a popover, under its trigger and flush with its right
 * edge. */
export function PopoverContent({
  className,
  ...props
}: PopoverPrimitive.Popup.Props) {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Positioner align="end" sideOffset={6} className="z-50">
        <PopoverPrimitive.Popup
          data-slot="popover-content"
          className={cn(
            "flex flex-col gap-2.5 rounded-lg border border-input bg-card p-3 text-sm shadow-overlay outline-none",
            className,
          )}
          {...props}
        />
      </PopoverPrimitive.Positioner>
    </PopoverPrimitive.Portal>
  );
}
