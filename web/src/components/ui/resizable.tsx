import * as ResizablePrimitive from "react-resizable-panels";
import { cn } from "@/lib/utils";

/** A row of panels with a `ResizableHandle` between each two. */
export const ResizablePanelGroup = ResizablePrimitive.Group;

export const ResizablePanel = ResizablePrimitive.Panel;

/** The line between two panels. A drag of the line, or an arrow key while
 * it has the focus, changes the widths of the two panels.
 *
 * The library starts a drag from 10 pixels around the line. The `before`
 * element covers those pixels, so the line gets a click in them too. */
export function ResizableHandle({
  className,
  ...props
}: ResizablePrimitive.SeparatorProps) {
  return (
    <ResizablePrimitive.Separator
      data-slot="resizable-handle"
      className={cn(
        "relative z-1 w-px bg-border outline-none before:absolute before:inset-y-0 before:-left-1 before:w-[9px] after:absolute after:inset-y-0 after:-left-px after:w-[3px] focus-visible:after:bg-ring data-[separator=active]:after:bg-input data-[separator=hover]:after:bg-input",
        className,
      )}
      {...props}
    />
  );
}
