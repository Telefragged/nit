import { useQuery } from "@tanstack/react-query";
import { Paperclip } from "lucide-react";
import { useState } from "react";
import { attachmentUrl, getAttachmentText } from "../../api/client";
import type { Reference } from "../../api/document.gen";
import { pathKind } from "../../lib/rendered";
import FileSection from "../diff/FileSection";
import { FileLoadNote, RenderedBody } from "../diff/RenderedView";
import Markdown from "../Markdown";

type Upload = Extract<Reference, { type: "upload" }>;

function UploadBody({ upload }: { upload: Upload }) {
  const { hash, name } = upload;
  const kind = pathKind(name);
  const text = useQuery({
    queryKey: ["attachment-text", hash, name],
    queryFn: () => getAttachmentText({ hash, name }),
    enabled: kind !== "image",
    staleTime: Infinity,
  });
  if (text.data === null)
    return (
      <div className="diff-note">
        <a href={attachmentUrl(upload)}>Download {name}</a>
      </div>
    );
  if (kind === "image" || kind === "html")
    return (
      <RenderedBody
        kind={kind}
        name={name}
        src={attachmentUrl(upload)}
        text={text.data}
        failed={text.isError}
      />
    );
  if (text.data === undefined) return <FileLoadNote failed={text.isError} />;
  if (kind === "markdown")
    return (
      <div className="rendered-markdown upload-markdown">
        <Markdown text={text.data} />
      </div>
    );
  return <pre className="upload-text">{text.data}</pre>;
}

/** An upload of a document, in the file section of the review page: an
 * image, an HTML page in its sandbox, a Markdown file as a page, and any
 * other text as lines. A file that is not text is a link to its bytes.
 * An upload is a file of no change, so its section has no thread and no
 * link. */
export default function UploadReference({ upload }: { upload: Upload }) {
  const [collapsed, setCollapsed] = useState(false);
  return (
    <div className="reference-diff">
      <FileSection
        collapsed={collapsed}
        onToggle={() => {
          setCollapsed(!collapsed);
        }}
        mark={<Paperclip size={12} aria-label="upload" />}
        title={upload.name}
      >
        {collapsed ? null : <UploadBody upload={upload} />}
      </FileSection>
    </div>
  );
}
