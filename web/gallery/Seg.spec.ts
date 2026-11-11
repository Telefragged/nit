import { test } from "@playwright/test";
import assert from "node:assert/strict";
import { clickNow, one } from "./act";

test("a press chooses an option, and a press on the chosen option keeps it", async ({
  mount,
}) => {
  const seg = await mount("components/Seg/Layout");
  const pressed = async () =>
    (await one(seg.locator('[aria-pressed="true"]'))).textContent();
  assert.equal(await pressed(), "Unified");
  await clickNow(seg.getByRole("button", { name: "Side-by-side" }));
  assert.equal(await pressed(), "Side-by-side");
  await clickNow(seg.getByRole("button", { name: "Side-by-side" }));
  assert.equal(await pressed(), "Side-by-side");
});
