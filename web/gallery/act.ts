import type { Locator, Page } from "@playwright/test";

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

/** The page half of `untilDom`, as source text: it resolves once
 * `test(arg)` holds. It runs the test now, then after each DOM mutation,
 * and never on a timer. */
const OBSERVE = `(test, arg) =>
  new Promise((resolve) => {
    const observer = new MutationObserver(check);
    function check() {
      if (!test(arg)) return;
      observer.disconnect();
      resolve();
    }
    observer.observe(document, {
      subtree: true,
      childList: true,
      attributes: true,
      characterData: true,
    });
    check();
  })`;

/** Resolves once `test(arg)` holds in the page. `test` runs in the page,
 * so it can use nothing of its module but `arg`. */
export const untilDom = (
  page: Page,
  test: (arg: never) => boolean,
  arg?: unknown,
) => page.evaluate(`(${OBSERVE})(${test.toString()}, ${JSON.stringify(arg)})`);

/** Resolves once `selector` matches an element of the page.
 *
 * A Select opens and closes its list after the click that toggles it, so a
 * step waits for the list or for the state of the head. */
export const untilSelector = (page: Page, selector: string) =>
  page.evaluate(
    `(${OBSERVE})((s) => !!document.querySelector(s), ${JSON.stringify(selector)})`,
  );

/** Resolves once the one element that `selector` matches is `width` pixels
 * wide. */
export const untilWidth = (page: Page, selector: string, width: number) =>
  page.evaluate(
    `(${OBSERVE})(
      ([s, w]) => document.querySelector(s)?.getBoundingClientRect().width === w,
      ${JSON.stringify([selector, width])},
    )`,
  );

/** Drags the one element that `locator` matches `dx` pixels to the right.
 * The pointer is `dy` pixels below the top of the element, or at its
 * middle when the caller gives no `dy`. */
export async function dragX(
  page: Page,
  locator: Locator,
  dx: number,
  dy?: number,
) {
  const box = await (await one(locator)).boundingBox();
  if (!box) throw new Error(`${locator.toString()} has no box`);
  const y = box.y + (dy ?? box.height / 2);
  await page.mouse.move(box.x, y);
  await page.mouse.down();
  await page.mouse.move(box.x + dx, y);
  await page.mouse.up();
}
