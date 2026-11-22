import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { MemoryRouter } from "react-router-dom";
import {
  reviewCtx,
  threadComment,
  uiThread,
} from "../../api/fixtures/builders";
import { ReviewContext } from "../../pages/reviewContext";
import CommentThread from "../CommentThread";
import FloatingPanel from "./FloatingPanel";
import type { RenderedAnchor } from "../../lib/comments";
import { type Outline, plainOutline } from "../../lib/rendered";
import ImageView from "./ImageView";

const SHOT = `data:image/svg+xml,${encodeURIComponent(`
<svg xmlns="http://www.w3.org/2000/svg" width="480" height="270">
  <rect width="480" height="270" fill="#f4f4f5"/>
  <rect x="120" y="30" width="240" height="210" rx="8" fill="#fff" stroke="#d4d4d8"/>
  <rect x="144" y="54" width="120" height="14" rx="3" fill="#27272a"/>
  <rect x="144" y="92" width="192" height="28" rx="4" fill="#fff" stroke="#a1a1aa"/>
  <rect x="144" y="134" width="192" height="28" rx="4" fill="#fff" stroke="#a1a1aa"/>
  <rect x="236" y="186" width="100" height="30" rx="4" fill="#2563eb"/>
</svg>`)}`;

const rect = (x: number, y: number, width: number, height: number) => ({
  region: { rect: { x, y, width, height } },
});

const OUTLINES: Outline[] = [
  { at: rect(230, 180, 112, 42), form: "open", threads: [3], active: false },
  plainOutline(rect(138, 48, 132, 26), "draft"),
  {
    at: rect(138, 128, 204, 40),
    form: "resolved",
    threads: [4],
    active: false,
  },
  plainOutline(rect(138, 86, 204, 40), "linked"),
];

export function Outlines() {
  return (
    <div style={{ width: 640 }}>
      <ImageView
        src={SHOT}
        alt="sign-in.png"
        outlines={OUTLINES}
        onLink={() => undefined}
        panel={null}
      />
    </div>
  );
}

export function Drag() {
  const [linked, setLinked] = useState<RenderedAnchor | null>(null);
  return (
    <div style={{ width: 640 }}>
      <ImageView
        src={SHOT}
        alt="sign-in.png"
        outlines={linked ? [plainOutline(linked, "linked")] : []}
        onLink={setLinked}
        panel={null}
      />
    </div>
  );
}

const queryClient = new QueryClient();

export function Panel() {
  const button = OUTLINES[0];
  if (button === undefined) throw new Error("no outline");
  return (
    <MemoryRouter>
      <QueryClientProvider client={queryClient}>
        <ReviewContext.Provider value={reviewCtx()}>
          <div style={{ width: 640, paddingBottom: 200 }}>
            <ImageView
              src={SHOT}
              alt="sign-in.png"
              outlines={[button]}
              onLink={() => undefined}
              panel={{
                at: button.at,
                content: (
                  <FloatingPanel
                    label="region 230,180 112x42"
                    onClose={() => undefined}
                  >
                    <CommentThread
                      thread={uiThread({
                        id: 3,
                        comments: [
                          threadComment(
                            30,
                            "The button has the color of a link. Use the primary button.",
                          ),
                        ],
                      })}
                    />
                  </FloatingPanel>
                ),
              }}
            />
          </div>
        </ReviewContext.Provider>
      </QueryClientProvider>
    </MemoryRouter>
  );
}
