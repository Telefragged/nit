import { clsx, type ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

const twMerge = extendTailwindMerge({
  extend: { theme: { shadow: ["overlay"] } },
});

/** Joins class names. Of two Tailwind classes for one property, the later
 * one stays. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export const errorMessage = (error: unknown) =>
  error instanceof Error ? error.message : String(error);
