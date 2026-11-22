import {
  type CSSProperties,
  type ReactNode,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { RenderedAnchor } from "../../lib/comments";
import {
  type FrameBox,
  type FrameMessage,
  type PageMessage,
} from "../../lib/framePicker";
import picker from "../../frame/picker?frame-script";
import type { Outline } from "../../lib/rendered";
import { shortcutKey } from "../../lib/shortcutKey";
import { useUnlinkOnSelection } from "../../lib/useUnlinkOnSelection";
import { OutlineBox, ScrollTargets } from "./ImageView";

const placed = (box: FrameBox): CSSProperties => ({
  left: box.x,
  top: box.y,
  width: box.width,
  height: box.height,
});

const under = (
  box: FrameBox,
  view: { width: number; height: number },
): CSSProperties => {
  const clamp = (v: number, max: number) => Math.min(max, Math.max(0, v));
  return {
    left: clamp(box.x, view.width),
    top: clamp(box.y + box.height, view.height),
  };
};

const elementOf = (o: Outline) => ("element" in o.at ? o.at.element : null);

export default function HtmlView({
  html,
  title,
  outlines,
  onLink,
  openEditor,
  reveal,
  panel,
}: {
  html: string;
  title: string;
  outlines: readonly Outline[];
  onLink: (at: RenderedAnchor | null) => void;
  openEditor: (at: RenderedAnchor) => boolean;
  reveal: string | null;
  panel: { at: RenderedAnchor; content: ReactNode } | null;
}) {
  const stage = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  // The parser moves a script after the end of the page into its body.
  const srcDoc = useMemo(
    () =>
      `${html}<script>${picker.replaceAll("</script", "<\\/script")}</script>`,
    [html],
  );
  const [boxes, setBoxes] = useState<ReadonlyMap<string, FrameBox | null>>();
  const [hover, setHover] = useState<FrameBox | null>(null);
  const [view, setView] = useState({ width: 0, height: 0 });
  const ready = boxes !== undefined;

  useUnlinkOnSelection(
    stage,
    outlines.some((o) => o.form === "linked"),
    () => {
      onLink(null);
    },
  );

  const post = (message: PageMessage) => {
    frame.current?.contentWindow?.postMessage({ nit: message }, "*");
  };

  const paths = [...new Set(outlines.flatMap((o) => elementOf(o) ?? []))].join(
    "\n",
  );
  useEffect(() => {
    if (ready) post({ type: "watch", paths: paths ? paths.split("\n") : [] });
  }, [ready, paths]);

  useLayoutEffect(() => {
    if (ready && reveal !== null) post({ type: "reveal", path: reveal });
  }, [ready, reveal]);

  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      if (e.source === null || e.source !== frame.current?.contentWindow)
        return;
      const m = (e.data as { nit?: FrameMessage } | null)?.nit;
      if (m?.type === "boxes" && Array.isArray(m.boxes)) {
        setBoxes(new Map(m.boxes));
        setView(m.view);
        setHover(m.hover);
      } else if (m?.type === "link" && typeof m.path === "string") {
        onLink({ element: m.path });
      } else if (m?.type === "key" && typeof m.key === "string") {
        const key = new KeyboardEvent("keydown", {
          key: m.key,
          shiftKey: m.shiftKey,
          ctrlKey: m.ctrlKey,
          altKey: m.altKey,
          metaKey: m.metaKey,
          cancelable: true,
        });
        if (m.selection !== null && shortcutKey(key) === "c") {
          if (openEditor({ element: m.selection })) post({ type: "unselect" });
          return;
        }
        window.dispatchEvent(key);
      }
    };
    window.addEventListener("message", onMessage);
    return () => {
      window.removeEventListener("message", onMessage);
    };
  }, [onLink, openEditor]);

  return (
    <div ref={stage} className="frame-stage" aria-busy={!ready}>
      {ready
        ? outlines.map((o, i) => <ScrollTargets key={i} outline={o} />)
        : null}
      <iframe
        ref={frame}
        className="rendered rendered-frame"
        sandbox="allow-scripts"
        title={title}
        srcDoc={srcDoc}
      />
      <div className="frame-marks">
        {hover ? (
          <div
            className="rendered-outline outline-hover"
            style={placed(hover)}
          />
        ) : null}
        {outlines.map((o, i) => {
          const path = elementOf(o);
          const box = path === null ? null : boxes?.get(path);
          return box ? (
            <OutlineBox
              key={i}
              outline={o}
              style={placed(box)}
              onLink={onLink}
            />
          ) : null;
        })}
      </div>
      {ready && panel && "element" in panel.at ? (
        <div
          className="place-panel"
          style={under(
            boxes.get(panel.at.element) ?? { x: 0, y: 0, width: 0, height: 0 },
            view,
          )}
        >
          {panel.content}
        </div>
      ) : null}
    </div>
  );
}
