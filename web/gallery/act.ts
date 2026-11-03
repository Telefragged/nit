import type { Locator } from "@playwright/test";

/** Returns `locator` after a check that it matches exactly one element.
 *
 * A step acts on a page that is already rendered, so a missing element is
 * a failure to report at once. A read through an unchecked locator waits
 * for the element. That wait has no timeout. */
export async function one(locator: Locator): Promise<Locator> {
  const count = await locator.count();
  if (count !== 1)
    throw new Error(`${locator.toString()} matched ${count} elements`);
  return locator;
}

/** Clicks the one element that `locator` matches, without Playwright's
 * retries for actionability.
 *
 * React commits the state update of a click handler before the task of
 * the click ends, so the DOM shows the update when this returns. */
export async function clickNow(locator: Locator) {
  await (await one(locator)).click({ force: true });
}
