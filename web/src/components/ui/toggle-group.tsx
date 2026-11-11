import { Toggle as TogglePrimitive } from "@base-ui/react/toggle";
import { ToggleGroup as ToggleGroupPrimitive } from "@base-ui/react/toggle-group";
import { cn } from "@/lib/utils";

export function ToggleGroup<Value extends string>({
  className,
  ...props
}: ToggleGroupPrimitive.Props<Value>) {
  return (
    <ToggleGroupPrimitive
      data-slot="toggle-group"
      className={cn(
        "inline-flex gap-0.5 rounded-md border border-border bg-sunken p-0.5",
        className,
      )}
      {...props}
    />
  );
}

export function ToggleGroupItem<Value extends string>({
  className,
  ...props
}: TogglePrimitive.Props<Value>) {
  return (
    <TogglePrimitive
      data-slot="toggle-group-item"
      className={cn(
        "inline-flex h-5.5 cursor-pointer items-center rounded-sm px-2.5 font-sans text-sm font-medium whitespace-nowrap text-muted-foreground outline-none select-none hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring data-pressed:bg-background data-pressed:text-foreground data-pressed:shadow-[0_0_0_1px_var(--color-border-strong)]",
        className,
      )}
      {...props}
    />
  );
}
