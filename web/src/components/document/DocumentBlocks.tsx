import { Pencil, Trash2 } from "lucide-react";
import { type ReactNode, useMemo } from "react";
import type { Reference } from "../../api/document.gen";
import { type Block, parseBlocks } from "../../lib/document";
import { blockSource, withBlock, withoutBlock } from "../../lib/documentText";
import { usePublishedChange } from "../../lib/useChangeStream";
import DocBlock from "../diff/DocBlock";
import TextEditor from "../TextEditor";
import { Button } from "../ui/button";
import ChangeCard from "./ChangeCard";
import FileReference from "./FileReference";
import UploadReference from "./UploadReference";

/** What the blocks of a page share: the open editor, and the edits of the
 * reviewer. A block is identified by its first line in the text. */
export interface DocumentActions {
  /** The block whose editor is open, or null. */
  editing: number | null;
  /** Opens the editor of a block. Null closes the open editor. */
  onEdit: (block: number | null) => void;
  /** Changes the text of the document to `change` of it. */
  edit: (change: (text: string) => string) => void;
}

function ReferenceView({
  id,
  reference,
}: {
  id: number;
  reference: Reference;
}) {
  const change = usePublishedChange(
    "change" in reference ? reference.change : null,
  );
  if (reference.type === "upload")
    return <UploadReference upload={reference} />;
  if (reference.type === "change")
    return (
      <ChangeCard
        number={reference.change}
        revision={change?.revisions.at(-1)}
      />
    );
  if (change === undefined)
    return <div className="skeleton" aria-busy style={{ height: 120 }} />;
  return <FileReference change={change} reference={reference} block={id} />;
}

function BlockTools({
  disabled,
  onEdit,
  onDelete,
}: {
  disabled: boolean;
  onEdit: () => void;
  onDelete: () => void;
}) {
  return (
    <div className="block-tools">
      <Button
        variant="ghost"
        size="icon"
        aria-label="Edit block"
        title="Edit block"
        disabled={disabled}
        onClick={onEdit}
      >
        <Pencil size={14} aria-hidden />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        className="hover:text-del"
        aria-label="Delete block"
        title="Delete block"
        disabled={disabled}
        onClick={() => {
          if (window.confirm("Delete this block?")) onDelete();
        }}
      >
        <Trash2 size={14} aria-hidden />
      </Button>
    </div>
  );
}

/** One block of a document. A block that is a reference renders the part
 * of the change that it shows, or its upload. Every other block renders
 * as Markdown. `references` contains the references of the text of the
 * block, by line (`referencesByLine`). `id` identifies the block on the
 * page. `change` is the class suffix of a block that a diff adds or
 * removes. `children` are the controls of the block. */
export function DocumentBlock({
  block,
  references,
  id,
  change = "",
  children,
}: {
  block: Block;
  references: ReadonlyMap<number, Reference>;
  id: number;
  change?: string;
  children?: ReactNode;
}) {
  const reference = references.get(block.lines[0] - 1);
  return (
    <div
      className={`document-block${reference ? "" : " rendered-markdown"}${change}`}
    >
      {children}
      {reference ? (
        <ReferenceView id={id} reference={reference} />
      ) : (
        <DocBlock block={block} side="new" />
      )}
    </div>
  );
}

/** The blocks of the Markdown `text` of a document, in order, each with
 * its controls. `references` contains the references of the text, by
 * line (`referencesByLine`). The open editor contains the source of its
 * block.
 *
 * The React key of a block is its source with the count of the equal
 * blocks above it. An edit above a block therefore keeps the state of
 * the block: a collapsed file, or an open draft. */
export default function DocumentBlocks({
  text,
  references,
  actions,
}: {
  text: string;
  references: ReadonlyMap<number, Reference>;
  actions: DocumentActions;
}) {
  const blocks = useMemo(() => {
    const lines = text.split("\n");
    const seen = new Map<string, number>();
    return parseBlocks(text).map((block) => {
      const source = blockSource(lines, block.lines);
      const above = seen.get(source) ?? 0;
      seen.set(source, above + 1);
      return { block, source, key: `${above}:${source}` };
    });
  }, [text]);
  return blocks.map(({ block, source, key }) => {
    const id = block.lines[0];
    const tools = block.fromSource ? (
      <BlockTools
        disabled={actions.editing !== null}
        onEdit={() => {
          actions.onEdit(id);
        }}
        onDelete={() => {
          actions.edit((text) => withoutBlock(text, block.lines));
        }}
      />
    ) : null;
    if (actions.editing !== id)
      return (
        <DocumentBlock key={key} block={block} references={references} id={id}>
          {tools}
        </DocumentBlock>
      );
    return (
      <div className="document-block" key={key}>
        {tools}
        <TextEditor
          initial={source}
          placeholder="Markdown…"
          saveLabel="Save"
          what="text"
          onSave={(source) => {
            actions.edit((text) => withBlock(text, block.lines, source));
            actions.onEdit(null);
          }}
          onCancel={() => {
            actions.onEdit(null);
          }}
        />
      </div>
    );
  });
}
