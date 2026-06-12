import { useEffect, useState } from "react";

/** Single confirmation point for every path that throws away editor text.
 * Returns true when discarding is OK: nothing dirty, or the user agreed. */
export function confirmDiscard(dirty: boolean): boolean {
  return !dirty || window.confirm("Discard unsaved comment?");
}

/** Plain draft editor: textarea + save/cancel. Used for new drafts, edits
 * and replies. Cancel (button or Escape) asks for confirmation before
 * discarding text that differs from what the editor opened with. Owners
 * whose UI can discard the editor by unmounting it (e.g. moving the inline
 * draft target) should mirror `onDirtyChange` and gate that path with
 * confirmDiscard. */
export default function CommentEditor({
  initial = "",
  placeholder = "Leave a comment…",
  saving,
  onSave,
  onCancel,
  onDirtyChange,
}: {
  initial?: string;
  placeholder?: string;
  saving: boolean;
  onSave: (body: string) => void;
  onCancel: () => void;
  /** Reports whether unsaved text would be lost; reset to false on unmount
   * and on a confirmed cancel (so unmount paths don't prompt twice). */
  onDirtyChange?: (dirty: boolean) => void;
}) {
  const [body, setBody] = useState(initial);
  const canSave = body.trim().length > 0 && !saving;
  const dirty = body.trim().length > 0 && body.trim() !== initial.trim();

  useEffect(() => {
    onDirtyChange?.(dirty);
    return () => onDirtyChange?.(false);
  }, [dirty, onDirtyChange]);

  function requestCancel() {
    if (!confirmDiscard(dirty)) return;
    onDirtyChange?.(false);
    onCancel();
  }

  return (
    <div className="editor">
      <textarea
        autoFocus
        value={body}
        placeholder={placeholder}
        onChange={(e) => setBody(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") requestCancel();
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && canSave) {
            onSave(body.trim());
          }
        }}
      />
      <div className="editor-actions">
        <button onClick={requestCancel} disabled={saving}>
          Cancel
        </button>
        <button
          className="btn-primary"
          onClick={() => onSave(body.trim())}
          disabled={!canSave}
        >
          {saving ? "Saving…" : "Save draft"}
        </button>
      </div>
    </div>
  );
}
