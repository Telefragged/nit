import { afterEach } from "vitest";

// Each test opens with an empty browser store. A page that remembers a view
// choice, for example the diff layout or the change graph's grouping, would
// otherwise carry the choice of one test into the next.
afterEach(() => {
  localStorage.clear();
});

// jsdom has no matchMedia, so a page that reads a media query throws
// without this stub. No query matches.
window.matchMedia = (media) => ({ matches: false, media }) as MediaQueryList;

// jsdom has no ResizeObserver. A group of resizable panels creates one.
// jsdom has no layout either, so no element ever changes its size.
const ignore = () => undefined;
window.ResizeObserver = class {
  observe = ignore;
  unobserve = ignore;
  disconnect = ignore;
};
