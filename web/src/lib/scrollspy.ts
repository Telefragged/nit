/**
 * Which file section is current for a scroll position: the last section
 * whose top sits at or above `threshold` (viewport-relative tops in
 * document order). Like a sticky header, a section stays current until the
 * next one reaches the line. null when the page is above the first section.
 */
export function activeIndexAt(
  sectionTops: number[],
  threshold: number,
): number | null {
  let active: number | null = null;
  for (const [i, top] of sectionTops.entries()) {
    if (top > threshold) break;
    active = i;
  }
  return active;
}

/** Smooth scrolling, unless the reviewer asks the system for reduced
 * motion. */
export const scrollBehavior = (): ScrollBehavior =>
  matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth";

export function whenPresent(
  root: Node,
  find: () => Element | null | undefined,
  found: (el: Element) => void,
): () => void {
  const el = find();
  if (el) {
    found(el);
    return () => undefined;
  }
  const observer = new MutationObserver(() => {
    const added = find();
    if (!added) return;
    observer.disconnect();
    found(added);
  });
  observer.observe(root, { childList: true, subtree: true });
  return () => {
    observer.disconnect();
  };
}
