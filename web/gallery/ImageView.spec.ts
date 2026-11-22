import { test } from "@playwright/test";
import assert from "node:assert/strict";
import { one, untilSelector } from "./act";

test("a drag across a scaled image links the rectangle in its pixels", async ({
  mount,
  page,
}) => {
  const root = await mount("components/diff/ImageView/Drag");
  const imageWidth = 480;
  const scale = 2;
  await page.addStyleTag({
    content: `.image-stage img { width: ${imageWidth / scale}px }`,
  });
  const box = await (await one(root.locator("img"))).boundingBox();
  assert.ok(box);
  await page.mouse.move(box.x + 10, box.y + 20);
  await page.mouse.down();
  await page.mouse.move(box.x + 60, box.y + 50);
  await page.mouse.up();
  await untilSelector(page, ".outline-linked");
  const outline = await (
    await one(root.locator(".outline-linked"))
  ).boundingBox();
  assert.ok(outline);
  assert.deepEqual(
    [outline.x - box.x, outline.y - box.y, outline.width, outline.height].map(
      Math.round,
    ),
    [10, 20, 50, 30],
    `a ${scale}x drag of 20,40 100x60 in the pixels of the image`,
  );
});
