// Instantiates the WebAssembly fold for vitest, which runs the suite in
// node. The glue's own loader fetches the asset URL vite emits, and no
// server answers that URL here, so hand it the bytes off disk.
//
// This file sits outside src/ on purpose: reading a file is a node
// concern, and src/ type-checks as the browser.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { initSync } from "./src/wasm/nit_wasm";

initSync(readFileSync(join(import.meta.dirname, "src/wasm/nit_wasm_bg.wasm")));
