import CommentEditor from "./CommentEditor";

const nothing = () => undefined;

/** The editor of a new comment. It has no text, so Save draft is off. */
export function New() {
  return (
    <div style={{ width: 840 }}>
      <CommentEditor saving={false} onSave={nothing} onCancel={nothing} />
    </div>
  );
}

/** A reply with text that also resolves its open thread. */
export function Reply() {
  return (
    <div style={{ width: 840 }}>
      <CommentEditor
        initial={
          "Both statements run in one transaction now.\n\n```rust\nlet tx = conn.transaction()?;\n```"
        }
        placeholder="Reply…"
        initialResolved
        resolvedFrom={false}
        saving={false}
        onSave={nothing}
        onCancel={nothing}
      />
    </div>
  );
}
