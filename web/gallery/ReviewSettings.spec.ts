import { test } from "@playwright/test";
import assert from "node:assert/strict";
import { clickNow, one, untilSelector } from "./act";

test("the cog opens the popover, a press changes a setting, and Escape closes it", async ({
  mount,
  page,
}) => {
  const root = await mount("components/ReviewSettings/Default");
  await clickNow(root.getByRole("button", { name: "Settings" }));
  await untilSelector(page, '[role="dialog"]');
  const popover = await one(
    page.getByRole("dialog", { name: "Review settings" }),
  );
  const split = popover.getByRole("button", { name: "Side-by-side" });
  await clickNow(split);
  assert.equal(await split.getAttribute("aria-pressed"), "true");
  await page.keyboard.press("Escape");
  await untilSelector(page, 'body:not(:has([role="dialog"]))');
  // The story has the changed setting, so the popover shows it again.
  await clickNow(root.getByRole("button", { name: "Settings" }));
  await untilSelector(page, '[role="dialog"]');
  assert.equal(
    await page
      .getByRole("button", { name: "Side-by-side" })
      .getAttribute("aria-pressed"),
    "true",
  );
});
