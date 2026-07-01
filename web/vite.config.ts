/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import wasm from "vite-plugin-wasm";
import { genWasmPlugin } from "./vite-plugin-gen-wasm";

// Dev mode: vite serves the frontend with HMR and proxies API calls to the
// rust backend (`cargo run -- serve`). Production: `vite build` output is
// served by the backend itself.
export default defineConfig({
  // genWasmPlugin(): generates src/wasm (gitignored) if missing, and in dev
  // watches crates/nit-wasm to regenerate + reload on change. wasm():
  // instantiates its `.wasm` import — the shared change fold ships as a
  // wasm-bindgen bundler module.
  plugins: [react(), wasm(), genWasmPlugin()],
  server: {
    proxy: {
      "/api": { target: "http://127.0.0.1:8877", ws: true },
    },
  },
  // Vitest (`npm test`): jsdom so component tests have a DOM; VITE_MOCK so
  // client.ts answers from the contract-true fixtures — the same canned
  // data the screenshot harness renders.
  test: {
    environment: "jsdom",
    env: { VITE_MOCK: "1" },
    setupFiles: ["./src/test-setup.ts"],
    // Must clear src/test-setup's asyncUtilTimeout so a load-stretched poll
    // isn't killed by the per-test cap before its findBy resolves.
    testTimeout: 20_000,
  },
});
