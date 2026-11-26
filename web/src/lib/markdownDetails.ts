import type { Parent, Root, RootContent, Text } from "mdast";

interface Summary extends Parent {
  type: "detailsSummary";
  children: Text[];
}

interface Details extends Parent {
  type: "details";
}

declare module "mdast" {
  interface RootContentMap {
    details: Details;
    detailsSummary: Summary;
  }
}

const OPENING =
  /^<details(\s+open)?\s*>\s*(?:<summary>([\s\S]*?)<\/summary>)?\s*$/i;
const CLOSING = /^<\/details\s*>\s*$/i;

function groupedUntilClosingBlockOrEnd(children: RootContent[]): RootContent[] {
  const top: RootContent[] = [];
  const unclosed: Details[] = [];
  for (const child of children) {
    const into = unclosed.at(-1)?.children ?? top;
    const opening = child.type === "html" && OPENING.exec(child.value);
    if (opening) {
      const [, isOpen, summary] = opening;
      const title = summary?.trim();
      const group: Details = {
        type: "details",
        position: child.position && { ...child.position },
        data: { hName: "details", hProperties: isOpen ? { open: true } : {} },
        children: [
          {
            type: "detailsSummary",
            data: { hName: "summary" },
            children: [
              {
                type: "text",
                value: title === undefined || title === "" ? "Details" : title,
                position: child.position,
              },
            ],
          },
        ],
      };
      into.push(group);
      unclosed.push(group);
      continue;
    }
    const group = unclosed.at(-1);
    if (group && child.type === "html" && CLOSING.test(child.value)) {
      if (group.position && child.position)
        group.position.end = child.position.end;
      unclosed.pop();
      continue;
    }
    if ("children" in child)
      (child as Parent).children = groupedUntilClosingBlockOrEnd(
        child.children as RootContent[],
      );
    into.push(child);
  }
  return top;
}

/** The remark plugin that renders a `<details>` block with an optional
 * `<summary>`, the blocks below it, and a `</details>` block as a
 * collapsible group. The summary is plain text. Every other HTML stays
 * out of the result. */
export function remarkDetails() {
  return (tree: Root) => {
    tree.children = groupedUntilClosingBlockOrEnd(tree.children);
  };
}
