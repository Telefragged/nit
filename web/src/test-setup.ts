import { afterEach } from "vitest";

// Each test opens with an empty browser store. A page that remembers a view
// choice, for example the diff layout or the change graph's grouping, would
// otherwise carry the choice of one test into the next.
afterEach(() => {
  localStorage.clear();
});

// jsdom has no matchMedia, so a call to matchMedia throws without this
// stub.
const ignore = () => undefined;
window.matchMedia = (media) =>
  ({
    matches: false,
    media,
    addEventListener: ignore,
    removeEventListener: ignore,
  }) as unknown as MediaQueryList;

// jsdom has no ResizeObserver. A group of resizable panels creates one.
// jsdom has no layout either, so no element ever changes its size.
window.ResizeObserver = class {
  observe = ignore;
  unobserve = ignore;
  disconnect = ignore;
};
