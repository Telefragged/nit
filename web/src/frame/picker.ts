import {
  elementAt,
  elementPath,
  type FrameMessage,
  type PageMessage,
} from "../lib/framePicker";

const post = (message: FrameMessage) => {
  window.parent.postMessage({ nit: message }, "*");
};
const boxOf = (el: Element | null) => {
  if (!el) return null;
  const { x, y, width, height } = el.getBoundingClientRect();
  return { x, y, width, height };
};
const oncePerFrame = (run: () => void) => {
  let queued = false;
  return () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      run();
    });
  };
};
let lastPosted = "";
const postIfChanged = (message: FrameMessage) => {
  const text = JSON.stringify(message);
  if (text === lastPosted) return;
  lastPosted = text;
  post(message);
};
let watched: string[] = [];
let hovered: Element | null = null;
const report = oncePerFrame(() => {
  postIfChanged({
    type: "boxes",
    view: { width: window.innerWidth, height: window.innerHeight },
    boxes: watched.map((p) => [p, boxOf(elementAt(document, p))]),
    hover: boxOf(hovered),
  });
});
const controls = "a, button, input, select, textarea, label, summary";
const editable = "input, select, textarea, [contenteditable]";
window.addEventListener("message", (e: MessageEvent) => {
  if (e.source !== window.parent) return;
  const m = (e.data as { nit?: PageMessage } | null)?.nit;
  if (m?.type === "watch") {
    watched = m.paths;
    report();
  } else if (m?.type === "reveal") {
    elementAt(document, m.path)?.scrollIntoView({ block: "center" });
  } else if (m?.type === "unselect") {
    window.getSelection()?.removeAllRanges();
  }
});
document.addEventListener("mouseover", (e) => {
  hovered = e.target instanceof Element ? e.target : null;
  report();
});
document.documentElement.addEventListener("mouseleave", () => {
  hovered = null;
  report();
});
document.addEventListener("click", (e) => {
  const el = e.target;
  if (!(el instanceof Element) || el.closest(controls)) return;
  if (e.button !== 0 || e.ctrlKey || e.metaKey || e.altKey) return;
  if (window.getSelection()?.isCollapsed === false) return;
  post({ type: "link", path: elementPath(el) });
});
document.addEventListener("keydown", (e) => {
  if (e.target instanceof Element && e.target.closest(editable)) return;
  const sel = window.getSelection();
  const node =
    sel && !sel.isCollapsed && sel.rangeCount > 0
      ? sel.getRangeAt(0).commonAncestorContainer
      : null;
  const el = node instanceof Element ? node : (node?.parentElement ?? null);
  post({
    type: "key",
    key: e.key,
    shiftKey: e.shiftKey,
    ctrlKey: e.ctrlKey,
    altKey: e.altKey,
    metaKey: e.metaKey,
    selection: el && elementPath(el),
  });
});
window.addEventListener("scroll", report, true);
window.addEventListener("resize", report);
new MutationObserver(report).observe(document, {
  subtree: true,
  childList: true,
  attributes: true,
  characterData: true,
});
report();
