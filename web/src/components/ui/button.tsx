import { Button as ButtonPrimitive } from "@base-ui/react/button";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex h-7 shrink-0 cursor-pointer items-center justify-center gap-1.5 rounded-md border border-transparent px-2.5 font-sans text-sm font-medium whitespace-nowrap outline-none select-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        // The one filled button of a view. Its action publishes the reviewer's work.
        primary:
          "bg-primary font-semibold text-primary-foreground hover:bg-primary/85",
        outline: "border-input bg-background text-foreground hover:bg-muted",
      },
    },
    defaultVariants: { variant: "outline" },
  },
);

export function Button({
  className,
  variant,
  ...props
}: ButtonPrimitive.Props & VariantProps<typeof buttonVariants>) {
  return (
    <ButtonPrimitive
      data-slot="button"
      className={cn(buttonVariants({ variant, className }))}
      {...props}
    />
  );
}
