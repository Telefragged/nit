import { createContext } from "react";
import type { DiffLink } from "./diffLink";

/** The place of one file reference that the page links to. One link
 * exists on a page, so that `c` drafts on the lines of one file. `block`
 * is the first line of the reference in the text. */
export interface BlockLink {
  block: number;
  link: DiffLink;
}

/** The link of a document page, and the function that sets it. File
 * references outside a document page have no link. */
export const DocumentLinkContext = createContext<{
  linked: BlockLink | null;
  onLink: (link: BlockLink) => void;
}>({ linked: null, onLink: () => undefined });
