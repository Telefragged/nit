// The shared fold, compiled to WebAssembly (crates/nit-wasm), with its
// module already instantiated.
//
// wasm-bindgen's `web` target leaves instantiation to the caller, and a
// wrapper called before `init()` throws. Awaiting it here makes every
// importer wait for it, so the fold is ready wherever it is called from.

import init from "../wasm/nit_wasm";

await init();

export * from "../wasm/nit_wasm";
