import { MessageSquare } from "lucide-react";
import { type CSSProperties, type ReactNode, useRef, useState } from "react";
import type { Rect } from "../../api/types";
import { type RenderedAnchor, sameRendered } from "../../lib/comments";
import { type Outline, plainOutline, rectBetween } from "../../lib/rendered";
import { plainClick } from "../../lib/shortcutKey";
import { useUnlinkOnSelection } from "../../lib/useUnlinkOnSelection";

interface Size {
  width: number;
  height: number;
}

const placed = (rect: Rect, size: Size): CSSProperties => ({
  left: `${(rect.x / size.width) * 100}%`,
  top: `${(rect.y / size.height) * 100}%`,
  width: `${(rect.width / size.width) * 100}%`,
  height: `${(rect.height / size.height) * 100}%`,
});

const under = (rect: Rect, size: Size): CSSProperties => ({
  left: `${(rect.x / size.width) * 100}%`,
  top: `${((rect.y + rect.height) / size.height) * 100}%`,
});

/** The outline of a place on a rendered file, with the threads at the
 * place. Each published thread has an element with its `data-thread`, so
 * that a link to the thread scrolls to the place. The button of a thread
 * outline calls `onLink`, which links the place and so opens its threads. */
function OutlineBox({
  outline,
  style,
  onLink,
}: {
  outline: Outline;
  style: CSSProperties;
  onLink: (at: RenderedAnchor) => void;
}) {
  const { form, threads, active, at } = outline;
  const threaded = form !== "linked" && form !== "editing";
  return (
    <div
      className={`rendered-outline outline-${form}${active ? " active" : ""}`}
      style={style}
    >
      {threads.map((id) => (
        <span key={id} className="outline-thread" data-thread={id} />
      ))}
      {threaded ? (
        <button
          type="button"
          className="outline-button"
          title="Show the threads here"
          aria-label="Show the threads here"
          onClick={() => {
            onLink(at);
          }}
        >
          <MessageSquare
            className={`thread-icon mark-${form}`}
            size={14}
            aria-hidden
          />
        </button>
      ) : null}
    </div>
  );
}

export default function ImageView({
  src,
  alt,
  outlines,
  onLink,
  panel,
}: {
  src: string | undefined;
  alt: string;
  outlines: readonly Outline[];
  onLink: (at: RenderedAnchor | null) => void;
  panel: { at: RenderedAnchor; content: ReactNode } | null;
}) {
  const stage = useRef<HTMLDivElement>(null);
  const [loaded, setLoaded] = useState<Size | "failed" | null>(null);
  const size =
    loaded === "failed" || loaded?.width === 0 || loaded?.height === 0
      ? null
      : loaded;
  const [drag, setDrag] = useState<Rect | null>(null);
  const dragged: RenderedAnchor | null = drag && { region: { rect: drag } };
  if (
    dragged &&
    outlines.some((o) => o.form === "linked" && sameRendered(o.at, dragged))
  )
    setDrag(null);

  const linked = outlines.some((o) => o.form === "linked");
  useUnlinkOnSelection(stage, linked, () => {
    onLink(null);
  });

  const pixel = (
    e: { clientX: number; clientY: number },
    img: Element,
    image: Size,
  ) => {
    const box = img.getBoundingClientRect();
    if (box.width === 0 || box.height === 0) return { x: 0, y: 0 };
    const clamp = (v: number, max: number) =>
      Math.min(max, Math.max(0, Math.round(v)));
    return {
      x: clamp(((e.clientX - box.left) * image.width) / box.width, image.width),
      y: clamp(
        ((e.clientY - box.top) * image.height) / box.height,
        image.height,
      ),
    };
  };

  const shown = dragged
    ? [
        ...outlines.filter((o) => o.form !== "linked"),
        plainOutline(dragged, "linked"),
      ]
    : outlines;

  return (
    <div className="rendered rendered-image">
      <div ref={stage} className="image-stage" aria-busy={loaded === null}>
        {/* A drag on the image is the only way to select a region in the
            web client. The keyboard reaches the outlines through their
            buttons. */}
        {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions */}
        <img
          src={src}
          alt={alt}
          draggable={false}
          onMouseDown={(e) => {
            if (!plainClick(e) || size === null) return;
            e.preventDefault();
            const img = e.currentTarget;
            const start = pixel(e, img, size);
            let rect: Rect | null = null;
            const move = (ev: MouseEvent) => {
              rect = rectBetween(start, pixel(ev, img, size));
              setDrag(rect);
            };
            document.addEventListener("mousemove", move);
            document.addEventListener(
              "mouseup",
              () => {
                document.removeEventListener("mousemove", move);
                if (rect) {
                  onLink({ region: { rect } });
                } else {
                  setDrag(null);
                  if (linked) onLink(null);
                }
              },
              { once: true },
            );
          }}
          onLoad={(e) => {
            const img = e.currentTarget;
            setLoaded({
              width: img.naturalWidth || img.clientWidth,
              height: img.naturalHeight || img.clientHeight,
            });
          }}
          onError={() => {
            setLoaded("failed");
          }}
        />
        {size
          ? shown.flatMap((o, i) =>
              "region" in o.at
                ? [
                    <OutlineBox
                      key={i}
                      outline={o}
                      style={placed(o.at.region.rect, size)}
                      onLink={onLink}
                    />,
                  ]
                : [],
            )
          : null}
        {size && panel && "region" in panel.at ? (
          <div
            className="place-panel"
            style={under(panel.at.region.rect, size)}
          >
            {panel.content}
          </div>
        ) : null}
      </div>
    </div>
  );
}
