import { useState } from "react";
import Shortcuts, { ShortcutList } from "./Shortcuts";

/** The button of the diffbar. A press opens the list. */
export function Closed() {
  const [open, setOpen] = useState(false);
  return <Shortcuts open={open} onOpenChange={setOpen} />;
}

/** The list. It centers itself in the viewport, so the story is as large
 * as the viewport. */
export function List() {
  return (
    <div className="h-screen w-screen">
      <ShortcutList onClose={() => undefined} />
    </div>
  );
}
