import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { confirmDiscard } from "../lib/confirmDiscard";
import { useAutosize } from "../lib/useAutosize";
import { Button } from "./ui/button";

/** Plain draft editor: a textarea and its save/cancel row in one box.
 * Used for new drafts, edits and replies. Cancel (button or Escape) opens
 * a confirmation prompt before it discards text that differs from the
 * text that the editor opened with. A parent component that can unmount
 * the editor, for example when the inline draft target moves, should
 * mirror `onDirtyChange` and call `confirmDiscard` before it unmounts the
 * editor.
 *
 * Pass `initialResolved` (gerrit-style thread resolution) to
 * show a Resolved checkbox defaulting to it; `onSave` then reports the
 * checkbox state, and the editor saves even with an empty body when the
 * checkbox alone changed (a resolve/reopen with no message). */
export default function CommentEditor({
  initial = "",
  placeholder = "Leave a comment…",
  saving,
  onSave,
  onCancel,
  onDirtyChange,
  initialResolved,
  resolvedFrom,
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
}) {
  const [body, setBody] = useState(initial);
  const [resolved, setResolved] = useState(initialResolved ?? false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  // Focus on open — the editor mounts when the user opens it — the
  // accessible alternative to the autoFocus attribute.
  useLayoutEffect(() => {
    textareaRef.current?.focus();
  }, []);

  const showResolve = initialResolved !== undefined;
  const resolveChanges =
    showResolve && resolved !== (resolvedFrom ?? initialResolved);
  const hasBody = body.trim().length > 0;
  const canSave = (hasBody || resolveChanges) && !saving;
  // Only typed text counts as discardable work — flipping the checkbox is
  // cheap to redo, so cancelling it never prompts.
  const dirty = hasBody && body.trim() !== initial.trim();

  useAutosize(textareaRef, body);

  useEffect(() => {
    onDirtyChange?.(dirty);
    return () => onDirtyChange?.(false);
  }, [dirty, onDirtyChange]);

  function save() {
    if (canSave) onSave(body.trim(), showResolve ? resolved : undefined);
  }

  function requestCancel() {
    if (!confirmDiscard(dirty)) return;
    onDirtyChange?.(false);
    onCancel();
  }

  return (
    <div className="editor">
      <textarea
        ref={textareaRef}
        value={body}
        placeholder={placeholder}
        onChange={(e) => {
          setBody(e.target.value);
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape") requestCancel();
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) save();
        }}
      />
      <div className="editor-actions">
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
        <span className="spacer" />
        <Button
          variant="ghost"
          size="sm"
          onClick={requestCancel}
          disabled={saving}
        >
          Cancel
        </Button>
        <Button size="sm" onClick={save} disabled={!canSave}>
          {saving ? "Saving…" : "Save draft"}
        </Button>
      </div>
    </div>
  );
}
