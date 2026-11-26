import { RotateCcwClock } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Button } from "../ui/button";
import { Collapsible, CollapsiblePanel } from "../ui/collapsible";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

const GAP_FROM_BUTTON_TO_TOP_OF_BAR = 9;

/** The history button of the document bar, with its popover above the
 * bar. The button expands and collapses the popover, whose content is
 * `children`, the versions.
 *
 * `shown` is the label of the versions on screen, or null for the live
 * document. While the page shows versions, the collapsed popover stays as
 * one line: the label, and the Live button, which calls `onLive`. A press
 * outside the popover leaves it as it is. */
export default function DocumentHistory({
  shown,
  onLive,
  children,
}: {
  shown: string | null;
  onLive: () => void;
  children: ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  const open = expanded || shown !== null;
  const [contentWhileOpen, setContentWhileOpen] = useState({ expanded, shown });
  if (
    open &&
    (contentWhileOpen.expanded !== expanded || contentWhileOpen.shown !== shown)
  )
    setContentWhileOpen({ expanded, shown });
  const content = open ? { expanded, shown } : contentWhileOpen;
  return (
    <Popover
      open={open}
      onOpenChange={(_, { reason }) => {
        if (reason === "trigger-press") setExpanded(!expanded);
        if (reason === "escape-key") setExpanded(false);
      }}
    >
      <PopoverTrigger
        render={
          <Button
            variant="ghost"
            size="icon"
            className="size-7"
            aria-label="Document history"
            aria-expanded={expanded}
            title="Document history"
          />
        }
      >
        <RotateCcwClock size={15} aria-hidden />
      </PopoverTrigger>
      <PopoverContent
        side="top"
        sideOffset={GAP_FROM_BUTTON_TO_TOP_OF_BAR}
        className="w-[340px] gap-0 shadow-none"
        aria-label="Document history"
      >
        <Collapsible open={content.expanded}>
          <CollapsiblePanel>
            <div className="mb-2.5 max-h-[50vh] overflow-y-auto">
              {children}
            </div>
          </CollapsiblePanel>
        </Collapsible>
        <div className="flex items-center justify-between gap-2">
          <span className="text-muted-foreground">{content.shown}</span>
          <Button
            size="sm"
            disabled={content.shown === null}
            title="Show the live document"
            onClick={onLive}
          >
            Live
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
