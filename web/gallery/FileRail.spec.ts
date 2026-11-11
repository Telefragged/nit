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

test("the chevron of a file has the form of its strongest thread", async ({
  mount,
}) => {
  const rail = await mount(DEFAULT);
  const chevron = async (path: string) => {
    const toggle = await one(
      rail.locator(`[data-path="${path}"] button.rail-toggle`),
    );
    return [
      /mark-\w+/.exec((await toggle.getAttribute("class")) ?? "")?.[0],
      await toggle.getAttribute("title"),
    ];
  };
  assert.deepEqual(await chevron("crates/nit/src/server.rs"), [
    "mark-open",
    "1 draft, 2 open",
  ]);
  assert.deepEqual(await chevron("crates/nit/src/scan.rs"), [
    "mark-resolved",
    "1 resolved",
  ]);
  assert.deepEqual(await chevron("docs/dev.md"), ["mark-draft", "1 draft"]);
});

test("a file row expands to a link per thread, with its state", async ({
  mount,
}) => {
  const rail = await mount(DEFAULT);
  await clickNow(rail.getByRole("button", { name: "comments on server.rs" }));
  const links = rail.locator(".rail-thread");
  assert.deepEqual(await links.allTextContents(), [
    "line 41r1",
    "range 88:4-90:12r0",
    "line 12r1",
  ]);
  const of = (attribute: string, inner = "") =>
    links
      .all()
      .then((all) =>
        Promise.all(
          all.map((link) =>
            (inner ? link.locator(inner) : link).getAttribute(attribute),
          ),
        ),
      );
  assert.deepEqual(await of("href"), [
    "/?thread=3",
    "/?thread=7",
    "/?file=crates/nit/src/server.rs&line=12",
  ]);
  assert.deepEqual(await of("title"), ["open", "open", "draft"]);
  assert.deepEqual(await of("class", ".rail-icon"), [
    "lucide lucide-message-square rail-icon mark-open",
    "lucide lucide-message-square rail-icon mark-open",
    "lucide lucide-message-square rail-icon mark-draft",
  ]);
});

test("selecting a file marks its row as current", async ({ mount }) => {
  const rail = await mount(DEFAULT);
  await clickNow(rail.getByRole("button", { name: "scan.rs", exact: true }));
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
