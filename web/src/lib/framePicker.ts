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
