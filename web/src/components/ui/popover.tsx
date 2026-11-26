import { Popover as PopoverPrimitive } from "@base-ui/react/popover";
import { cn } from "@/lib/utils";

export const Popover = PopoverPrimitive.Root;

export const PopoverTrigger = PopoverPrimitive.Trigger;

export const PopoverTitle = PopoverPrimitive.Title;

/** The card of a popover, flush with the right edge of its trigger. It
 * is under the trigger, or on `side` of it, `sideOffset` pixels away. */
export function PopoverContent({
  className,
  side,
  sideOffset = 6,
  ...props
}: PopoverPrimitive.Popup.Props &
  Pick<PopoverPrimitive.Positioner.Props, "side" | "sideOffset">) {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Positioner
        align="end"
        side={side}
        sideOffset={sideOffset}
        className="z-(--z-overlay)"
      >
        <PopoverPrimitive.Popup
          data-slot="popover-content"
          className={cn(
            "flex flex-col gap-2.5 rounded-lg border border-input bg-card p-3 text-sm shadow-overlay transition-opacity duration-150 outline-none data-ending-style:opacity-0 data-starting-style:opacity-0",
            className,
          )}
          {...props}
        />
      </PopoverPrimitive.Positioner>
    </PopoverPrimitive.Portal>
  );
}
