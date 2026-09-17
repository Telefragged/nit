import { use, type ReactNode } from "react";
import { ready } from "../lib/wasm";

/** Holds its children until the WebAssembly fold is callable.
 *
 * Suspends, so it needs a `Suspense` boundary above it. Wrap any route
 * whose render path reaches the fold. */
export default function FoldReady({ children }: { children: ReactNode }) {
  use(ready);
  return children;
}
