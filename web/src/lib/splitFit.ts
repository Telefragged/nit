import { createContext } from "react";

/** The number of characters that fit on one line of a code column of the
 * split layout. It is null outside a diff column that measures it, and
 * before the first measurement. */
export const SplitFit = createContext<number | null>(null);
