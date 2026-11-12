import { test, type Locator, type Page } from "@playwright/test";
import assert from "node:assert/strict";
import { one } from "./act";

/** Presses the mouse button at the left edge of the gutter cell `cell`
 * of the row that shows new line `line`, and returns whether the row then
 * shows as linked. */
async function pressGutter(
  page: Page,
  file: Locator,
  line: number,
  cell: number,
) {
  const code = await one(file.locator(`.code[data-new="${line}"]`));
  const row = code.locator("xpath=..");
  const box = await (await one(row.locator(".g").nth(cell))).boundingBox();
  assert.ok(box);
  await page.mouse.move(box.x + 1, box.y + box.height - 1);
  await page.mouse.down();
  const linked = /\blinked\b/.test((await code.getAttribute("class")) ?? "");
  await page.mouse.up();
  return linked;
}

test("a press at the edge of a gutter cell links its line", async ({
  mount,
  page,
}) => {
  const file = await mount("components/diff/DiffFileView/Unified");
  // Each row is now taller than its number, as a row with wrapped code is.
  await page.addStyleTag({
    content: ".line-row .code { padding-block: 20px }",
  });
  // Line 18 is unchanged, so each of its two cells has a number.
  assert.equal(await pressGutter(page, file, 18, 0), true);
  assert.equal(await pressGutter(page, file, 18, 1), true);
});

test("a press in the empty gutter cell of an added line links the line", async ({
  mount,
  page,
}) => {
  const file = await mount("components/diff/DiffFileView/Unified");
  assert.equal(await pressGutter(page, file, 22, 0), true);
});
