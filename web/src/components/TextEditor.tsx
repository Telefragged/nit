import {
  type ReactNode,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { confirmDiscard } from "../lib/confirmDiscard";
import { useAutosize } from "../lib/useAutosize";
import { Button } from "./ui/button";

/** A textarea and its save and cancel row in one box. Cancel (button or
 * Escape) opens a confirmation prompt before it discards text that
 * differs from the text that the editor opened with. A parent component
 * that can unmount the editor, for example when the inline draft target
 * moves, should mirror `onDirtyChange` and call `confirmDiscard` before
 * it unmounts the editor. `children` are the controls at the start of the
 * button row. */
export default function TextEditor({
  initial = "",
  placeholder,
  saving = false,
  saveLabel,
  what,
  savesEmpty = false,
  onSave,
  onCancel,
  onDirtyChange,
  children,
}: {
  initial?: string;
  placeholder: string;
  saving?: boolean;
  saveLabel: string;
  /** The word for the text in the confirmation prompt (`confirmDiscard`). */
  what?: string;
  /** Saves also when the text is empty. */
  savesEmpty?: boolean;
  onSave: (text: string) => void;
  onCancel: () => void;
  /** Reports whether unsaved text would be lost; reset to false on unmount
   * and on a confirmed cancel (so unmount paths don't prompt twice). */
  onDirtyChange?: (dirty: boolean) => void;
  children?: ReactNode;
}) {
  const [body, setBody] = useState(initial);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    textareaRef.current?.focus();
  }, []);

  const hasBody = body.trim().length > 0;
  const canSave = (hasBody || savesEmpty) && !saving;
  const dirty = hasBody && body.trim() !== initial.trim();

  useAutosize(textareaRef, body);

  useEffect(() => {
    onDirtyChange?.(dirty);
    return () => onDirtyChange?.(false);
  }, [dirty, onDirtyChange]);

  function save() {
    if (canSave) onSave(body.trim());
  }

  function requestCancel() {
    if (!confirmDiscard(dirty, what)) return;
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
        {children}
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
          {saving ? "Saving…" : saveLabel}
        </Button>
      </div>
    </div>
  );
}
