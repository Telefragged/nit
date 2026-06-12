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

/** File sections start collapsed; captures that show diff contents open
 * them all via the rail's toggle first. */
const expandAllFiles = async (page) => {
  await page.getByRole("button", { name: "expand all" }).click();
  await page.waitForTimeout(100);
};

/**
 * Page states to capture. `actions` runs after load to put the page into a
 * specific state (toggles, editors, error paths). Add an entry whenever a
 * page or significant state is added.
 */
const captures = [
  { name: "dashboard", path: "/" },
  { name: "chain-waiting", path: "/chains/1" },
  // Chain 2 is partial (push --partial) and carries a scan error: covers the
  // PARTIAL badge and the validation-error banner in the chain header here
  // and on its dashboard row above.
  { name: "chain-agents-turn", path: "/chains/2" },
  // Change 11 has last_reviewed_revision 1 < latest 2 → interdiff by default.
  { name: "review-interdiff", path: "/changes/11" },
  // Long review cover message expanded via the "more" toggle. Viewport-only
  // so the header text stays at full resolution.
  {
    name: "review-cover-expanded",
    path: "/changes/11",
    fullPage: false,
    actions: async (page) => {
      await page.locator(".review-item .review-more").click();
      await page.waitForTimeout(100);
    },
  },
  // Chain strip collapsed: status dots (current ringed) and the N/M toggle
  // inline at the right end of the header's meta line.
  {
    name: "review-chain-strip",
    path: "/changes/11?against=base",
    fullPage: false,
  },
  // Chain strip expanded: the panel lists the whole chain in normal flow
  // on its own meta-line row, pushing the content below it down.
  {
    name: "review-chain-expanded",
    path: "/changes/11",
    fullPage: false,
    actions: async (page) => {
      await page.locator(".chain-strip-toggle").click();
      await page.waitForSelector(".chain-strip-panel");
    },
  },
  {
    name: "review-full-unified",
    path: "/changes/11?against=base",
    actions: expandAllFiles,
  },
  // Collapsed-by-default file sections: only the synthetic commit message
  // starts expanded, the code files are header-only rows.
  { name: "review-files-collapsed", path: "/changes/11?against=base" },
  // Mixed state via a rail click: the selected file expands and is
  // scrolled to; its collapsed neighbor stays collapsed.
  {
    name: "review-files-mixed",
    path: "/changes/11?against=base",
    actions: async (page) => {
      await page.locator('.rail-item[title="src/auth/store.rs"]').click();
      await page.waitForTimeout(300);
    },
  },
  // The synthetic "Commit message" file with its resolved inline thread.
  {
    name: "review-commit-msg",
    path: "/changes/11?against=base",
    fullPage: false,
  },
  {
    name: "review-split",
    path: "/changes/11?against=base",
    actions: async (page) => {
      await expandAllFiles(page);
      await page.getByRole("button", { name: "Side-by-side" }).click();
      await page.waitForTimeout(200);
    },
  },
  // Scroll spy: scrolled into the middle of the third file, so its header
  // is pinned under the diffbar and the rail highlights it (no click —
  // expand-all first, or the collapsed page has nothing to scroll into).
  {
    name: "review-scroll-spy",
    path: "/changes/11?against=base",
    fullPage: false,
    actions: async (page) => {
      await expandAllFiles(page);
      await page.evaluate(() => {
        const el = document.getElementById("file-2");
        window.scrollTo(
          0,
          window.scrollY + el.getBoundingClientRect().top - 40,
        );
      });
      await page.waitForTimeout(250); // rAF measure + react re-render
    },
  },
  // Scrolled mid-file: the sticky file header must pin flush under the
  // diffbar as one opaque box (regression view for the sticky offsets in
  // styles.css — diffbar height and .file-header top must agree). Expands
  // first: a collapsed file is header-only, so nothing would pin.
  {
    name: "review-scrolled-sticky",
    path: "/changes/11?against=base",
    fullPage: false,
    actions: async (page) => {
      await expandAllFiles(page);
      await page.evaluate(() => {
        const sec = document.getElementById("file-1");
        window.scrollTo(0, sec.offsetTop + 200);
      });
      await page.waitForTimeout(200);
    },
  },
  // Old revision selected: full diff of r1, threads at their written lines.
  {
    name: "review-rev1",
    path: "/changes/11?revision=1",
    actions: expandAllFiles,
  },
  // Explicit interdiff picked via the base dropdown (no "since your
  // review" hint) — exercises select → URL → refetch end to end.
  {
    name: "review-interdiff-picked",
    path: "/changes/11?against=base",
    fullPage: false,
    actions: async (page) => {
      await page.getByLabel("Diff base").selectOption("1");
      await page.waitForTimeout(200);
    },
  },
  {
    name: "review-draft-editor",
    path: "/changes/11?against=base",
    actions: async (page) => {
      await expandAllFiles(page);
      await page
        .locator("td.code", { hasText: "self.store.revoke_family" })
        .first()
        .click();
      await page.waitForSelector("textarea");
      await page
        .locator("textarea")
        .fill("Should revoke_family also bump the metrics counter?");
    },
  },
  // Published range threads: the multi-line selection on rotate.rs and
  // the partial-line one on the commit message render tinted
  // (docs/api.md "Range comments").
  {
    name: "review-range-comments",
    path: "/changes/11?against=base",
    actions: expandAllFiles,
  },
  // Selecting diff text and pressing c: the inline editor opens on the
  // selection's last line with the pending range tinted "active".
  {
    name: "review-range-draft",
    path: "/changes/11?against=base",
    actions: async (page) => {
      await expandAllFiles(page);
      await page.evaluate(() => {
        const texts = [...document.querySelectorAll("td.code .code-text")];
        const cell = (needle) =>
          texts.find((t) => t.textContent.includes(needle));
        // hljs splits lines into token spans; find the text node (and
        // offset) carrying the needle for a partial-line boundary.
        const point = (root, needle, atEnd) => {
          const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
          for (let t = walker.nextNode(); t; t = walker.nextNode()) {
            const i = t.data.indexOf(needle);
            if (i >= 0) return [t, atEnd ? i + needle.length : i];
          }
          return [root, 0];
        };
        const range = document.createRange();
        range.setStart(...point(cell("if entry.rotated_at"), "entry", false));
        range.setEnd(
          ...point(cell("RotateError::ReuseDetected)"), "ReuseDetected", true),
        );
        const sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(range);
      });
      await page.keyboard.press("c");
      await page.waitForSelector("textarea");
      await page
        .locator("textarea")
        .fill("This whole reuse branch deserves its own unit test.");
    },
  },
  // Reply modal opened via the `a` shortcut, cover message typed. Typed
  // key by key (not fill, which replaces content) so a shortcut keystroke
  // leaking into the autofocused textarea would show up in the capture.
  {
    name: "review-modal",
    path: "/changes/11?against=base",
    fullPage: false,
    actions: async (page) => {
      await page.keyboard.press("a");
      await page
        .getByPlaceholder("Cover message (published with the verdict)…")
        .pressSequentially("Nice cleanup — two nits inline, otherwise ready.");
    },
  },
  // Submitting against a stale revision → 409 inside the modal, which
  // stays open with drafts + message kept.
  {
    name: "review-409",
    path: "/changes/11?revision=1",
    fullPage: false,
    actions: async (page) => {
      await page.getByRole("button", { name: "Review (a)" }).click();
      await page
        .getByPlaceholder("Cover message (published with the verdict)…")
        .fill("Looks good overall, minor nits.");
      await page.getByRole("button", { name: "Comment", exact: true }).click();
      await page.waitForSelector(".review-conflict");
    },
  },
  // Rename + binary file in one diff.
  {
    name: "review-binary-rename",
    path: "/changes/12",
    actions: expandAllFiles,
  },
];

/**
 * Against a real server (NIT_BASE_URL) the mock-fixture ids above don't
 * exist; discover what does and capture it generically. Detailed UI states
 * stay covered by mock mode — live mode verifies real backend data renders.
 */
async function liveCaptures(baseUrl) {
  const res = await fetch(`${baseUrl}/api/chains`);
  const { chains } = await res.json();
  const caps = [{ name: "live-dashboard", path: "/" }];
  for (const chain of chains) {
    caps.push({ name: `live-chain-${chain.id}`, path: `/chains/${chain.id}` });
    for (const ch of chain.changes.slice(0, 2)) {
      caps.push({ name: `live-change-${ch.id}`, path: `/changes/${ch.id}` });
    }
  }
  return caps;
}

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
    const list = process.env.NIT_BASE_URL
      ? await liveCaptures(baseUrl)
      : captures;

    const browser = await chromium.launch();
    const context = await browser.newContext({
      viewport: { width: 1440, height: 900 },
      colorScheme: "dark",
      reducedMotion: "reduce",
    });
    // Keep captures order-independent (e.g. the persisted diff layout).
    await context.addInitScript(() => localStorage.clear());

    for (const cap of list) {
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (err) => errors.push(String(err)));
      await page.goto(baseUrl + cap.path, { waitUntil: "networkidle" });
      if (cap.actions) await cap.actions(page);
      // Fixed elements repeat confusingly in full-page captures; pin the
      // review bar to the end of the document instead. Viewport captures
      // keep it fixed so bar + modal render as they really stack.
      if (cap.fullPage ?? true) {
        await page.addStyleTag({
          content: ".review-bar { position: static !important; }",
        });
      }
      await page.waitForTimeout(150); // settle fonts/highlighting
      const file = resolve(outDir, `${cap.name}.png`);
      await page.screenshot({ path: file, fullPage: cap.fullPage ?? true });
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
