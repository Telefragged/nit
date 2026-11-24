import { useState } from "react";
import TextEditor from "./TextEditor";

/** Plain draft editor: a `TextEditor` for new drafts, edits and replies.
 *
 * Pass `initialResolved` (gerrit-style thread resolution) to
 * show a Resolved checkbox defaulting to it; `onSave` then reports the
 * checkbox state, and the editor saves even with an empty body when the
 * checkbox alone changed (a resolve/reopen with no message), unless
 * `needsBody` is set. */
export default function CommentEditor({
  initial,
  placeholder = "Leave a comment…",
  saving,
  onSave,
  onCancel,
  onDirtyChange,
  initialResolved,
  resolvedFrom,
  needsBody = false,
}: {
  initial?: string;
  placeholder?: string;
  saving: boolean;
  onSave: (body: string, resolved?: boolean) => void;
  onCancel: () => void;
  /** Reports whether unsaved text would be lost; reset to false on unmount
   * and on a confirmed cancel (so unmount paths don't prompt twice). */
  onDirtyChange?: (dirty: boolean) => void;
  /** Undefined hides the resolve checkbox; a boolean shows it at that default. */
  initialResolved?: boolean;
  /** The thread's current resolution, against which a bare checkbox change is
   * judged savable. Defaults to `initialResolved` (e.g. Resolve opens the box
   * pre-checked but the thread is still open, so saving it is a real change). */
  resolvedFrom?: boolean;
  /** Requires a body to save, also when the checkbox changed. A comment
   * that opens a thread requires one. */
  needsBody?: boolean;
}) {
  const [resolved, setResolved] = useState(initialResolved ?? false);
  const showResolve = initialResolved !== undefined;
  const resolveChanges =
    showResolve && resolved !== (resolvedFrom ?? initialResolved);

  return (
    <TextEditor
      initial={initial}
      placeholder={placeholder}
      saving={saving}
      saveLabel="Save draft"
      savesEmpty={resolveChanges && !needsBody}
      onSave={(body) => {
        onSave(body, showResolve ? resolved : undefined);
      }}
      onCancel={onCancel}
      onDirtyChange={onDirtyChange}
    >
      {showResolve ? (
        <label className="check-label">
          <input
            type="checkbox"
            checked={resolved}
            onChange={(e) => {
              setResolved(e.target.checked);
            }}
          />
          Resolved
        </label>
      ) : null}
    </TextEditor>
  );
}
