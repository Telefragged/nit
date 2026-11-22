import { useState } from "react";
import { type RenderedAnchor, renderedLabel } from "../../lib/comments";
import { type Outline, plainOutline } from "../../lib/rendered";
import HtmlView from "./HtmlView";

const PAGE = `<!doctype html>
<body style="font-family: sans-serif; padding: 16px; background: #fff; color: #18181b">
  <h2>POST /auth/refresh</h2>
  <p>Rotates a refresh token.</p>
  <p>Returns 401 when the token is unknown.</p>
  <table border="1" cellpadding="6">
    <tr><th>Field</th><th>Type</th></tr>
    <tr><td>token</td><td>string</td></tr>
  </table>
</body>`;

const OUTLINES: Outline[] = [
  {
    at: { element: "/html/body/h2" },
    form: "open",
    threads: [3],
    active: false,
  },
  {
    at: { element: "/html/body/table" },
    form: "resolved",
    threads: [4],
    active: false,
  },
  plainOutline({ element: "/html/body/p[2]" }, "linked"),
];

export function Outlines() {
  return (
    <div style={{ width: 640 }}>
      <HtmlView
        html={PAGE}
        title="index.html"
        outlines={OUTLINES}
        onLink={() => undefined}
        openEditor={() => false}
        reveal={null}
        panel={null}
      />
    </div>
  );
}

export function Pick() {
  const [linked, setLinked] = useState<RenderedAnchor | null>(null);
  const [draft, setDraft] = useState<RenderedAnchor | null>(null);
  return (
    <div style={{ width: 640 }}>
      <HtmlView
        html={PAGE}
        title="index.html"
        outlines={linked ? [plainOutline(linked, "linked")] : []}
        onLink={setLinked}
        openEditor={(at) => {
          setDraft(at);
          return true;
        }}
        reveal={null}
        panel={null}
      />
      <output data-draft={draft ? "" : undefined}>
        {draft ? renderedLabel(draft) : "no draft"}
      </output>
    </div>
  );
}
