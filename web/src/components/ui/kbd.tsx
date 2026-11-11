import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

/** The key of a keyboard shortcut. */
export function Kbd({ className, ...props }: ComponentProps<"kbd">) {
  return (
    <kbd
      data-slot="kbd"
      className={cn(
        "inline-flex h-4 min-w-4 items-center justify-center rounded-sm border border-input bg-card px-1 font-mono text-xs font-normal text-muted-foreground select-none",
        className,
      )}
      {...props}
    />
  );
}
