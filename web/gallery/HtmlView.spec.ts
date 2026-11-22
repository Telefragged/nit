import { test, type Locator, type Page } from "@playwright/test";
import assert from "node:assert/strict";
import { one, untilIdle, untilSelector } from "./act";

async function mountPick(mount: (id: string) => Promise<unknown>, page: Page) {
  await mount("components/diff/HtmlView/Pick");
  await untilIdle(page);
  return page.frameLocator("iframe");
}

async function boxOf(page: Page, locator: Locator) {
  const el = await (await one(locator)).boundingBox();
  const at = await (await one(page.locator("iframe"))).boundingBox();
  assert.ok(el && at);
  return [el.x - at.x, el.y - at.y, el.width, el.height].map(Math.round);
}

test("a click on an element of the page outlines it", async ({
  mount,
  page,
}) => {
  const frame = await mountPick(mount, page);
  await (await one(frame.locator("p").nth(1))).click();
  await untilSelector(page, ".outline-linked");
  assert.deepEqual(
    await boxOf(page, page.locator(".outline-linked")),
    await boxOf(page, frame.locator("p:nth-of-type(2)")),
  );
});

test("the element under the pointer has an outline", async ({
  mount,
  page,
}) => {
  const frame = await mountPick(mount, page);
  await (await one(frame.locator("h2"))).hover();
  await untilSelector(page, ".outline-hover");
  assert.deepEqual(
    await boxOf(page, page.locator(".outline-hover")),
    await boxOf(page, frame.locator("h2")),
  );
});

test("c over selected text drafts on the element that contains it", async ({
  mount,
  page,
}) => {
  const frame = await mountPick(mount, page);
  await (await one(frame.locator("td").first())).dblclick();
  await page.keyboard.press("c");
  await untilSelector(page, "output[data-draft]");
  assert.equal(
    await page.locator("output").textContent(),
    "element /html/body/table/tbody/tr[2]/td",
  );
});
