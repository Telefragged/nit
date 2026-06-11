import { useState } from "react";

/** Plain draft editor: textarea + save/cancel. Used for new drafts, edits
 * and replies. */
export default function CommentEditor({
  initial = "",
  placeholder = "Leave a comment…",
  saving,
  onSave,
  onCancel,
}: {
  initial?: string;
  placeholder?: string;
  saving: boolean;
  onSave: (body: string) => void;
  onCancel: () => void;
}) {
  const [body, setBody] = useState(initial);
  const canSave = body.trim().length > 0 && !saving;

  return (
    <div className="editor">
      <textarea
        autoFocus
        value={body}
        placeholder={placeholder}
        onChange={(e) => setBody(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onCancel();
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && canSave) {
            onSave(body.trim());
          }
        }}
      />
      <div className="editor-actions">
        <button onClick={onCancel} disabled={saving}>
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
