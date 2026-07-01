import { existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import type { Plugin } from "vite";

const WASM_MARKER = "src/wasm/nit_wasm.js";
const NIT_WASM_SRC = "../crates/nit-wasm";

function genWasm() {
  execFileSync("gen-wasm", { stdio: "inherit" });
}

// Regenerates the wasm-bindgen glue (crates/nit-wasm) via the devShell's
// `gen-wasm`. vite.config.ts is shared by dev/build/vitest/screenshots, so
// this alone covers those; `tsc`/`eslint` never run through vite and still
// need the package.json `pre*` guard (see ensure-wasm).
export function genWasmPlugin(): Plugin {
  return {
    name: "gen-wasm",
    buildStart() {
      if (!existsSync(WASM_MARKER)) genWasm();
    },
    configureServer(server) {
      // Dev-only: rebuild and force a reload when the Rust source changes,
      // so editing crates/nit-wasm behaves like editing any other source.
      server.watcher.add(NIT_WASM_SRC);
      server.watcher.on("change", (file) => {
        if (!file.includes("/nit-wasm/")) return;
        genWasm();
        server.ws.send({ type: "full-reload" });
      });
    },
  };
}
