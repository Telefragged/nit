import { test } from "@playwright/test";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const dir = process.env.NIT_GALLERY ?? "";
const gallery = readFileSync(join(dir, "index.html"), "utf8");
const ids = JSON.parse(
  readFileSync(join(dir, "stories.json"), "utf8"),
) as string[];

// Each page is the whole gallery with the story id in `data-story`, so nit
// can show the page as an attachment without other files.
for (const id of ids) {
  test(`captures ${id}`, async ({ page, mount }, testInfo) => {
    const root = await mount(id);
    const out = join(testInfo.project.outputDir, id);
    // root.screenshot() would first retry until the root is visible and
    // stable.
    const clip = await root.boundingBox();
    if (!clip) throw new Error(`${id} rendered no box`);
    await page.screenshot({ path: `${out}.png`, clip, fullPage: true });
    writeFileSync(
      `${out}.html`,
      gallery.replace("<body>", `<body data-story="${id}">`),
    );
  });
}
