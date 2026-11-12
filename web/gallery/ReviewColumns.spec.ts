import { test, type Page } from "@playwright/test";
import assert from "node:assert/strict";
import { dragX, one, untilWidth } from "./act";

const STORY = "components/ReviewColumns/Columns";
const RAIL = "[data-panel]:first-child";

/** The width of the rail that the browser saved. */
const saved = (page: Page) =>
  page.evaluate(`localStorage.getItem("nit.rail-width")`);

test("the browser saves the width after a drag and after a double-click of the line", async ({
  mount,
  page,
}) => {
  const columns = await mount(STORY);
  const line = columns.getByRole("separator");
  await untilWidth(page, RAIL, 284);
  await dragX(page, line, 100);
  await untilWidth(page, RAIL, 384);
  assert.equal(await saved(page), "384");
  // The double-click is beside the line, and it sets the default width.
  const box = await (await one(line)).boundingBox();
  assert.ok(box);
  await page.mouse.dblclick(box.x + 3, box.y + box.height / 2);
  await untilWidth(page, RAIL, 284);
  assert.equal(await saved(page), "284");
});

test("the rail is never narrower than 200 pixels", async ({ mount, page }) => {
  const columns = await mount(STORY);
  await untilWidth(page, RAIL, 284);
  await dragX(page, columns.getByRole("separator"), -250);
  await untilWidth(page, RAIL, 200);
});
