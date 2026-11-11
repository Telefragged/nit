import { test } from "@playwright/test";
import assert from "node:assert/strict";
import { clickNow, one, untilSelector } from "./act";

test("the button opens the list, and Escape closes it", async ({
  mount,
  page,
}) => {
  const root = await mount("components/Shortcuts/Closed");
  await clickNow(root.getByRole("button", { name: "Shortcuts" }));
  await untilSelector(page, '[role="dialog"]');
  const list = await one(page.getByRole("dialog"));
  assert.equal(
    await list.getAttribute("aria-labelledby"),
    await (await one(list.getByRole("heading"))).getAttribute("id"),
  );
  await page.keyboard.press("Escape");
  await untilSelector(page, 'body:not(:has([role="dialog"]))');
  await one(root.getByRole("button", { name: "Shortcuts" }));
});
