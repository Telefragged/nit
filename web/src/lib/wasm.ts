// The shared fold, compiled to WebAssembly (crates/nit-wasm).
//
// wasm-bindgen's `web` target leaves instantiation to the caller, and a
// wrapper called before `init()` throws. Importing this module starts the
// download; `ready` says when the fold is callable. Nothing here awaits,
// so the app mounts and its first requests go out while the module is
// still arriving.

import init from "../wasm/nit_wasm";

/** Resolves once the module is instantiated and the fold is callable. */
export const ready = init();

export * from "../wasm/nit_wasm";
