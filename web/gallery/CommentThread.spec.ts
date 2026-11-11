import { test, type Locator } from "@playwright/test";
import assert from "node:assert/strict";
import { clickNow, one } from "./act";

/** Where the toggle is, and where the tail of the head ends and has its
 * middle. The state is the last part of the tail. Both stay in place when
 * a thread opens or closes. */
async function headPlaces(thread: Locator) {
  const toggle = await (
    await one(
      thread.getByRole("button", { name: /^(Expand|Collapse) thread$/ }),
    )
  ).boundingBox();
  const tail = await (await one(thread.locator(".thread-tail"))).boundingBox();
  if (!toggle || !tail) throw new Error("the head has no box");
  // In whole pixels: the box of an italic label has a fraction of a pixel.
  return {
    toggle,
    tailEnd: [
      Math.round(tail.x + tail.width),
      Math.round(tail.y + tail.height / 2),
    ],
  };
}

test("a resolved thread starts as one row and its toggle expands it", async ({
  mount,
}) => {
  const thread = await mount("components/CommentThread/Resolved");
  const head = await one(thread.locator(".comment-head"));
  assert.match((await head.textContent()) ?? "", /2 commentsResolved$/);
  assert.equal(await thread.getByRole("button", { name: "Reopen" }).count(), 0);
  await clickNow(thread.getByRole("button", { name: "Expand thread" }));
  await one(thread.getByRole("button", { name: "Reopen" }));
  await clickNow(thread.getByRole("button", { name: "Collapse thread" }));
  await one(thread.getByRole("button", { name: "Expand thread" }));
});

test("an open thread shows in full and collapses to one row", async ({
  mount,
}) => {
  const thread = await mount("components/CommentThread/Open");
  await one(thread.getByRole("button", { name: "Resolve" }));
  await clickNow(thread.getByRole("button", { name: "Collapse thread" }));
  const head = await one(thread.locator(".comment-head"));
  assert.match((await head.textContent()) ?? "", /2 commentsOpen$/);
});

test("the toggle and the state keep their place when a thread opens", async ({
  mount,
}) => {
  for (const story of ["Resolved", "Open"]) {
    const thread = await mount(`components/CommentThread/${story}`);
    const before = await headPlaces(thread);
    await clickNow(
      thread.getByRole("button", { name: /^(Expand|Collapse) thread$/ }),
    );
    assert.deepEqual(await headPlaces(thread), before, story);
  }
});

test("a drafted resolve collapses the thread and names its state", async ({
  mount,
}) => {
  const thread = await mount("components/CommentThread/ResolveDrafted");
  const state = await one(thread.locator(".comment-head .status"));
  assert.equal(await state.textContent(), "Draft");
  assert.equal(
    await state.getAttribute("title"),
    "Resolved once you submit the review",
  );
});
