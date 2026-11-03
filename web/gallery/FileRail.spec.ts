import { test } from "@playwright/test";
import assert from "node:assert/strict";
import { clickNow, one } from "./act";

const DEFAULT = "components/diff/FileRail/Default";

test("a click on a directory row hides its files", async ({ mount }) => {
  const rail = await mount(DEFAULT);
  const dir = await one(
    rail.getByRole("button", { name: "crates / nit / src" }),
  );
  await clickNow(dir);
  assert.equal(await dir.getAttribute("aria-expanded"), "false");
  assert.equal(await rail.getByText("server.rs").count(), 0);
});

test("a file row shows the counts of its drafts and published threads", async ({
  mount,
}) => {
  const rail = await mount(DEFAULT);
  const counts = await one(
    rail.locator('[data-path="crates/nit/src/server.rs"] .rail-counts'),
  );
  assert.equal(await counts.textContent(), "1d2c+12−3M");
});

test("a file row expands to a link per published thread", async ({ mount }) => {
  const rail = await mount(DEFAULT);
  await clickNow(rail.getByRole("button", { name: "comments on server.rs" }));
  const links = rail.locator(".rail-thread");
  assert.deepEqual(await links.allTextContents(), [
    "line 41r1",
    "range 88:4-90:12r0",
  ]);
  const hrefs = (await links.all()).map((link) => link.getAttribute("href"));
  assert.deepEqual(await Promise.all(hrefs), ["/?thread=3", "/?thread=7"]);
});

test("selecting a file marks its row as current", async ({ mount }) => {
  const rail = await mount(DEFAULT);
  await clickNow(rail.getByText("scan.rs"));
  const current = await one(rail.locator('[aria-current="true"]'));
  assert.equal(
    await current.getAttribute("data-path"),
    "crates/nit/src/scan.rs",
  );
});

test("the title row switches between expand all and collapse all", async ({
  mount,
}) => {
  const rail = await mount(DEFAULT);
  await clickNow(rail.getByRole("button", { name: "expand all" }));
  await one(rail.getByRole("button", { name: "collapse all" }));
});

test("while the diff loads, the rail shows 0 files and no button", async ({
  mount,
}) => {
  const rail = await mount("components/diff/FileRail/Empty");
  const title = await one(rail.locator(".rail-title"));
  assert.equal(await title.textContent(), "0 files");
  assert.equal(await rail.getByRole("button").count(), 0);
});
