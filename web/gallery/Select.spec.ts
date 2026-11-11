import { test } from "@playwright/test";
import assert from "node:assert/strict";
import { clickNow, one, untilSelector } from "./act";

const LIST = '[role="listbox"]';
const CLOSED = '[role="combobox"][aria-expanded="false"]';

test("the list names each option, and a press chooses one", async ({
  mount,
  page,
}) => {
  const picker = await mount("components/Select/Tags");
  const head = await one(picker.getByRole("combobox", { name: "Tag" }));
  assert.equal(await head.textContent(), "session-id auth-rotation");
  await clickNow(head);
  await untilSelector(page, LIST);
  assert.deepEqual(await page.getByRole("option").allTextContents(), [
    "branch track/auth-rotation",
    "session-id auth-rotation",
    "worktree .worktrees/auth-rotation",
  ]);
  await clickNow(page.getByRole("option", { name: "branch" }));
  await untilSelector(page, CLOSED);
  assert.equal(await head.textContent(), "branch track/auth-rotation");
});

test("Escape closes the list and keeps the value", async ({ mount, page }) => {
  const picker = await mount("components/Select/Revisions");
  const head = await one(picker.getByRole("combobox", { name: "Revision" }));
  await clickNow(head);
  await untilSelector(page, LIST);
  await page.keyboard.press("Escape");
  await untilSelector(page, CLOSED);
  assert.equal(await head.textContent(), "r1 3 comments");
});

test("a picker without an option is disabled", async ({ mount }) => {
  const picker = await mount("components/Select/Empty");
  const head = await one(picker.getByRole("combobox", { name: "Tag" }));
  assert.equal(await head.isDisabled(), true);
  assert.equal(await head.textContent(), "no tags");
});
