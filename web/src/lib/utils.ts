import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** Joins class names. Of two Tailwind classes for one property, the later
 * one stays. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
