// Vitest setup: size the async-polling ceilings for load. A `nix flake check`
// runs every crate's build/clippy/test concurrently, so a jsdom test can be
// badly CPU-starved — the mock's real setTimeout latency (fixtures/index) and
// React's renders then land far later than on an idle box. testing-library's
// 1000ms `asyncUtilTimeout` default is too tight for that and flakes the
// rail-load `findBy`s. findBy/waitFor poll until the state holds, so a generous
// ceiling is a true fix, not a probability shift: the assertion still resolves
// the instant the element appears — the ceiling only has to clear the
// worst-case load-stretched completion, never the happy path.
import { configure } from "@testing-library/react";

export const ASYNC_TIMEOUT_MS = 10_000;

configure({ asyncUtilTimeout: ASYNC_TIMEOUT_MS });
