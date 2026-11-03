// Playwright runs this global setup before it loads the spec files and
// starts the workers. So capture.spec.ts can declare a test per story, and
// the workers inherit NIT_GALLERY.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { build, type Plugin } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";
import { storyId } from "./storyId";

const src = join(import.meta.dirname, "../src");

export default async function setup() {
  const ids: string[] = [];
  const listStories: Plugin = {
    name: "list-stories",
    buildEnd() {
      for (const id of this.getModuleIds()) {
        if (!id.endsWith(".story.tsx")) continue;
        const names = this.getModuleInfo(id)?.exports ?? [];
        ids.push(...names.map((name) => storyId(relative(src, id), name)));
      }
    },
  };
  const result = await build({
    root: import.meta.dirname,
    configFile: join(import.meta.dirname, "../vite.config.ts"),
    plugins: [viteSingleFile(), listStories],
    logLevel: "warn",
    build: { write: false },
  });
  const page =
    "output" in result
      ? result.output.find((o) => o.fileName === "index.html")
      : undefined;
  if (page?.type !== "asset")
    throw new Error("the build emitted no index.html");
  const dir = mkdtempSync(join(tmpdir(), "nit-gallery-"));
  writeFileSync(join(dir, "index.html"), page.source);
  writeFileSync(join(dir, "stories.json"), JSON.stringify(ids));
  process.env.NIT_GALLERY = dir;
  return () => {
    rmSync(dir, { recursive: true });
  };
}
