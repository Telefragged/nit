import { useState, type ReactNode } from "react";
import { usePanelRef } from "react-resizable-panels";
import { readSaved, writeSaved } from "../lib/storage";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "./ui/resizable";

const KEY = "nit.rail-width";

const DEFAULT_WIDTH = 284;

// The page scrolls in the window. A panel that clips its content is a
// scroll container. The sticky rail and the sticky file headers would then
// stick to that panel, not to the window.
const UNCLIPPED = { overflow: "visible" } as const;

function savedWidth(): number {
  const saved = readSaved(KEY);
  return typeof saved === "number" ? saved : DEFAULT_WIDTH;
}

/** The two columns of the review page: `rail` at the left, and `children`
 * at the right.
 *
 * The reviewer changes the width of the rail with the line between the
 * columns. A double-click on the line sets the default width again. The
 * browser saves the width in pixels, so the rail keeps its width in a
 * window of another size, up to half of the width of the two columns. */
export default function ReviewColumns({
  rail,
  children,
}: {
  rail: ReactNode;
  children: ReactNode;
}) {
  const railRef = usePanelRef();
  const [width] = useState(savedWidth);
  return (
    <ResizablePanelGroup
      style={UNCLIPPED}
      onLayoutChanged={(_, { isUserInteraction }) => {
        const size = railRef.current?.getSize();
        if (isUserInteraction && size) writeSaved(KEY, size.inPixels);
      }}
    >
      <ResizablePanel
        panelRef={railRef}
        defaultSize={width}
        minSize={200}
        maxSize="50%"
        groupResizeBehavior="preserve-pixel-size"
        style={UNCLIPPED}
      >
        <aside className="review-sidebar">{rail}</aside>
      </ResizablePanel>
      {/* The library's own double-click sets `defaultSize`, which is the
          width that the page loaded with. */}
      <ResizableHandle
        aria-label="Width of the file rail"
        disableDoubleClick
        onDoubleClick={() => {
          railRef.current?.resize(DEFAULT_WIDTH);
          writeSaved(KEY, DEFAULT_WIDTH);
        }}
      />
      <ResizablePanel style={UNCLIPPED}>{children}</ResizablePanel>
    </ResizablePanelGroup>
  );
}
