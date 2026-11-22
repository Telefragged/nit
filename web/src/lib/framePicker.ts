export interface FrameBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type FrameMessage =
  | {
      type: "boxes";
      view: { width: number; height: number };
      boxes: [path: string, box: FrameBox | null][];
      hover: FrameBox | null;
    }
  | { type: "link"; path: string }
  | {
      type: "key";
      key: string;
      shiftKey: boolean;
      ctrlKey: boolean;
      altKey: boolean;
      metaKey: boolean;
      selection: string | null;
    };

export type PageMessage =
  | { type: "watch"; paths: string[] }
  | { type: "reveal"; path: string }
  | { type: "unselect" };

export function elementPath(el: Element): string {
  const plain = /^[A-Za-z][A-Za-z0-9-]*$/;
  const chain: Element[] = [];
  for (let node: Element | null = el; node; node = node.parentElement)
    chain.unshift(node);
  const firstUnnamable = chain.findIndex((node) => !plain.test(node.localName));
  const nearestNamable =
    firstUnnamable === -1 ? chain : chain.slice(0, firstUnnamable);
  const steps = nearestNamable.map((node) => {
    let n = 1;
    for (
      let s = node.previousElementSibling;
      s !== null;
      s = s.previousElementSibling
    )
      if (s.localName === node.localName) n++;
    return n > 1 ? `${node.localName}[${n}]` : node.localName;
  });
  return `/${steps.join("/")}`;
}

export function elementAt(doc: Document, path: string): Element | null {
  let node: Element | null = null;
  for (const step of path.split("/").slice(1)) {
    const m = /^([A-Za-z][A-Za-z0-9-]*)(?:\[(\d+)\])?$/.exec(step);
    if (!m) return null;
    const pool: Element[] = node ? [...node.children] : [doc.documentElement];
    const named = pool.filter((c) => c.localName === m[1]);
    node = named[Number(m[2] ?? 1) - 1] ?? null;
    if (node === null) return null;
  }
  return node;
}

function picker(
  pathOf: (el: Element) => string,
  find: (doc: Document, path: string) => Element | null,
) {
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
      boxes: watched.map((p) => [p, boxOf(find(document, p))]),
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
      find(document, m.path)?.scrollIntoView({ block: "center" });
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
    post({ type: "link", path: pathOf(el) });
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
      selection: el && pathOf(el),
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
}

/** `html` with the script of the frame added at its end, where the parser
 * moves it into the body. */
export const withPicker = (html: string) =>
  `${html}<script>(${picker.toString()})(${elementPath.toString()}, ${elementAt.toString()})</script>`;
