// The file tree of a diff: its paths nested into directories, with
// directories before files at each level.

/** A directory of the tree. `name` holds every segment of a chain of
 * directories that each hold only the next one, joined by `/`. */
interface TreeDir {
  kind: "dir";
  name: string;
  /** The full path of the directory, ending in `/`. No other directory of
   * the tree has the same path. For a merged chain, it is the path of the
   * last directory in the chain. */
  path: string;
  children: TreeNode[];
}

/** A file of the tree, by its index in the paths the tree is built from. */
interface TreeFile {
  kind: "file";
  name: string;
  index: number;
}

export type TreeNode = TreeDir | TreeFile;

const collator = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: "base",
});

/** Directories before files, then by name: case-insensitive, and with
 * digit runs compared as numbers, so `a9` precedes `a10`. */
function compareNodes(a: TreeNode, b: TreeNode): number {
  if (a.kind !== b.kind) return a.kind === "dir" ? -1 : 1;
  return collator.compare(a.name, b.name);
}

/** Sorts the tree, and merges each directory that holds only one
 * directory into it. */
function flatten(node: TreeNode): TreeNode {
  if (node.kind === "file") return node;
  let dir = node;
  while (dir.children.length === 1 && dir.children[0]?.kind === "dir") {
    const only = dir.children[0];
    dir = { ...only, name: `${dir.name}/${only.name}` };
  }
  return { ...dir, children: dir.children.sort(compareNodes).map(flatten) };
}

/** Nests `paths` into a sorted tree. Each file keeps the index of its path
 * in `paths`, and a null path has no file in the tree. */
export function buildTree(paths: readonly (string | null)[]): TreeNode[] {
  const root: TreeDir = { kind: "dir", name: "", path: "", children: [] };
  paths.forEach((path, index) => {
    if (path === null) return;
    const parts = path.split("/");
    let dir = root;
    for (const part of parts.slice(0, -1)) {
      const at = `${dir.path}${part}/`;
      let next = dir.children.find(
        (c): c is TreeDir => c.kind === "dir" && c.path === at,
      );
      if (next === undefined) {
        next = { kind: "dir", name: part, path: at, children: [] };
        dir.children.push(next);
      }
      dir = next;
    }
    dir.children.push({ kind: "file", name: parts.at(-1) ?? path, index });
  });
  return root.children.sort(compareNodes).map(flatten);
}

/** The indexes of the files of `tree`, in the order the tree lists them. */
export function fileOrder(tree: readonly TreeNode[]): number[] {
  return tree.flatMap((n) =>
    n.kind === "file" ? [n.index] : fileOrder(n.children),
  );
}
