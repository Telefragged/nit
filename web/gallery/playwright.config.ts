import { defineConfig } from "@playwright/test";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

// Each worker loads this file again, after setup.ts sets the variable.
const gallery = process.env.NIT_GALLERY;

export default defineConfig({
  globalSetup: "./setup.ts",
  // 0 turns the timeout off. A test waits on events, never on a clock.
  timeout: 0,
  fullyParallel: true,
  // Playwright empties this directory before each run, so a run removes the
  // captures of a renamed or removed story.
  outputDir: "../../screenshots/gallery",
  use: {
    baseURL: gallery && pathToFileURL(join(gallery, "index.html")).href,
    viewport: { width: 1440, height: 900 },
    colorScheme: "dark",
    reducedMotion: "reduce",
    launchOptions: {
      // The same flags as screenshots/capture.mjs, for the same sandbox.
      args: process.env.NIT_SCREENSHOT_NO_SANDBOX
        ? ["--no-sandbox", "--disable-dev-shm-usage"]
        : [],
      timeout: 0,
    },
  },
});
