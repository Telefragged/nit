/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev mode: vite serves the frontend with HMR and proxies API calls to the
// rust backend (`cargo run -- serve`). Production: `vite build` output is
// served by the backend itself.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": "http://127.0.0.1:8877",
    },
  },
  // Vitest (`npm test`): jsdom so component tests have a DOM; VITE_MOCK so
  // client.ts answers from the contract-true fixtures — the same canned
  // data the screenshot harness renders.
  test: {
    environment: "jsdom",
    env: { VITE_MOCK: "1" },
  },
});
