import type { AnchorKind } from "../lib/comments";
import { Button } from "./ui/button";

/** The button that opens the editor of a new thread on the whole change or
 * on a whole file. */
export default function NewThreadButton({
  anchor,
  onClick,
  className,
}: {
  anchor: Exclude<AnchorKind, "line">;
  onClick: () => void;
  className?: string;
}) {
  return (
    <Button variant="ghost" className={className} onClick={onClick}>
      Comment on {anchor}
    </Button>
  );
}
