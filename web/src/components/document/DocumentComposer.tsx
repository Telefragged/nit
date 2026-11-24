import { useMutation } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { uploadAttachment } from "../../api/client";
import { uploadMarkdown } from "../../lib/reference";
import { pathKind } from "../../lib/rendered";
import TextEditor from "../TextEditor";
import { Button } from "../ui/button";

/** The controls that add to the end of a document: Markdown from an
 * editor, and a reference to an upload from a file. `onAppend` receives
 * the Markdown to add. */
export default function DocumentComposer({
  onAppend,
}: {
  onAppend: (markdown: string) => void;
}) {
  const [writing, setWriting] = useState(false);
  const file = useRef<HTMLInputElement>(null);

  const upload = useMutation({
    mutationFn: (chosen: File) => uploadAttachment(chosen),
    onSuccess: ({ hash }, { name }) => {
      onAppend(uploadMarkdown(hash, name, pathKind(name) === "image"));
    },
  });

  return (
    <div className="document-composer">
      {writing ? (
        <TextEditor
          placeholder="Markdown…"
          saveLabel="Add"
          what="text"
          onSave={(text) => {
            onAppend(text);
            setWriting(false);
          }}
          onCancel={() => {
            setWriting(false);
          }}
        />
      ) : null}
      <div className="composer-row">
        <Button
          disabled={writing}
          onClick={() => {
            setWriting(true);
          }}
        >
          Add text
        </Button>
        <input
          ref={file}
          type="file"
          hidden
          aria-label="File to upload"
          onChange={(e) => {
            const chosen = e.target.files?.[0];
            if (chosen) upload.mutate(chosen);
            // The same file can be chosen again.
            e.target.value = "";
          }}
        />
        <Button
          disabled={upload.isPending}
          onClick={() => {
            file.current?.click();
          }}
        >
          {upload.isPending ? "Uploading…" : "Upload file"}
        </Button>
      </div>
      {upload.error ? (
        <div className="composer-note" role="status">
          {upload.error.message}
        </div>
      ) : null}
    </div>
  );
}
