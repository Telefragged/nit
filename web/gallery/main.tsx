// The gallery page (docs/dev.md "Component gallery"). Playwright's `mount`
// fixture calls `window.mount` here to render one story into #root.

import { StrictMode, type ComponentType } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { ready } from "../src/lib/wasm";
import "../src/styles/index.css";
import { storyId } from "./storyId";

const modules = import.meta.glob<Record<string, ComponentType>>(
  "../src/**/*.story.tsx",
  { eager: true },
);

/** Each story by its id (`storyId`). */
const stories = new Map<string, ComponentType>(
  Object.entries(modules).flatMap(([file, exports]) => {
    const path = file.replace(/^\.\.\/src\//, "");
    return Object.entries(exports).map(
      ([name, story]) => [storyId(path, name), story] as const,
    );
  }),
);

const rootEl = document.getElementById("root");
if (!rootEl) throw new Error("missing #root element");
/** The last error that a story threw. */
let thrown: Error | undefined;
// Every call renders into this root, so the `update()` of the fixture
// re-renders the story with new props and keeps its state.
const root = createRoot(rootEl, {
  onUncaughtError: (e) => {
    thrown =
      e instanceof Error ? e : new Error("the story threw", { cause: e });
  },
});

async function mount({ story, props }: { story: string; props?: object }) {
  const Story = stories.get(story);
  if (!Story) throw new Error(`no story ${story}`);
  // A story can call the fold while it renders.
  await ready;
  // flushSync commits the render before it returns. As a result, `mount`
  // returns after the commit, and it throws the error of a story that throws.
  flushSync(() => {
    root.render(
      <StrictMode>
        <Story {...props} />
      </StrictMode>,
    );
  });
  if (thrown) throw thrown;
}

Object.assign(window, { mount });

// capture.spec.ts writes a page per story with the story id in `data-story`.
const preset = document.body.dataset.story;
if (preset !== undefined) void mount({ story: preset });
