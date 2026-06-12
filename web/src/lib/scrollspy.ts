// Pure scroll-spy decision, kept out of components so it stays testable.

/**
 * Which file section is current for a scroll position: the last section
 * whose top sits at or above `threshold` (viewport-relative tops in
 * document order). Like a sticky header, a section stays current until the
 * next one reaches the line. null when the page is above the first section
 * — no file has reached the sticky line, so nothing should highlight.
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
