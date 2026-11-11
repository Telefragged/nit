import { test } from "@playwright/test";
import assert from "node:assert/strict";
import { one } from "./act";

test("the text box grows with its lines and has no scroll of its own", async ({
  mount,
  page,
}) => {
  const editor = await mount("components/CommentEditor/New");
  const text = await one(editor.getByRole("textbox"));
  const height = async () => (await text.boundingBox())?.height ?? 0;
  const empty = await height();
  await text.fill(Array.from({ length: 12 }, (_, i) => `line ${i}`).join("\n"));
  assert.ok((await height()) > empty);
  // Source text, because this project has no DOM types.
  const overflow: number = await page.evaluate(
    `(() => {
      const el = document.querySelector(".editor textarea");
      return el.scrollHeight - el.clientHeight;
    })()`,
  );
  assert.equal(overflow, 0);
});
