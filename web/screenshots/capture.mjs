// Screenshot harness — renders every page/state into PNGs so agents (and
// humans) can see the UI. `npm run screenshots` from web/.
//
// Default mode: starts a vite dev server with VITE_MOCK=1 (canned fixtures,
// no backend) and captures against it. Set NIT_BASE_URL to capture against
// an already-running server (e.g. the real rust backend) instead.
//
// Output: <repo root>/screenshots/*.png (gitignored).
//
// Browsers come from the nix devShell ($PLAYWRIGHT_BROWSERS_PATH); the
// @playwright/test npm version is pinned to the driver version. Never run
// `playwright install` here.

import { chromium } from "@playwright/test";
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const webDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = resolve(webDir, "../screenshots");
const PORT = 5187;

/**
 * Page states to capture. `actions` runs after load to put the page into a
 * specific state (toggles, editors, error paths). Add an entry whenever a
 * page or significant state is added.
 */
const captures = [
  { name: "dashboard", path: "/" },
  { name: "chain-warnings", path: "/chains/1" },
  { name: "chain-agents-turn", path: "/chains/2" },
  { name: "review-placeholder", path: "/changes/11" },
];

async function waitForServer(url, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(url);
      if (res.ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`server at ${url} did not come up in ${timeoutMs}ms`);
}

async function main() {
  let baseUrl = process.env.NIT_BASE_URL;
  let server = null;

  if (!baseUrl) {
    baseUrl = `http://127.0.0.1:${PORT}`;
    server = spawn(
      resolve(webDir, "node_modules/.bin/vite"),
      ["--port", String(PORT), "--strictPort"],
      {
        cwd: webDir,
        env: { ...process.env, VITE_MOCK: "1" },
        stdio: ["ignore", "pipe", "inherit"],
      },
    );
    server.stdout.resume(); // drain, keep quiet
  }

  try {
    await waitForServer(baseUrl);
    mkdirSync(outDir, { recursive: true });

    const browser = await chromium.launch();
    const context = await browser.newContext({
      viewport: { width: 1440, height: 900 },
      colorScheme: "dark",
      reducedMotion: "reduce",
    });

    for (const cap of captures) {
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (err) => errors.push(String(err)));
      await page.goto(baseUrl + cap.path, { waitUntil: "networkidle" });
      if (cap.actions) await cap.actions(page);
      await page.waitForTimeout(150); // settle fonts/highlighting
      const file = resolve(outDir, `${cap.name}.png`);
      await page.screenshot({ path: file, fullPage: true });
      console.log(
        `captured ${cap.name}.png${errors.length ? `  PAGE ERRORS: ${errors.join("; ")}` : ""}`,
      );
      if (errors.length) process.exitCode = 1;
      await page.close();
    }

    await browser.close();
    console.log(`done → ${outDir}`);
  } finally {
    if (server) server.kill("SIGTERM");
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
