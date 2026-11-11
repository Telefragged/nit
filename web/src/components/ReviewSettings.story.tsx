import { useState } from "react";
import { DEFAULTS } from "../lib/reviewSettings";
import ReviewSettingsMenu from "./ReviewSettings";

/** The cog at the right of a bar. A press opens the popover under it, and
 * the settings are state of the story. */
export function Default() {
  const [settings, setSettings] = useState(DEFAULTS);
  return (
    <div className="flex h-[190px] w-[400px] items-start justify-end">
      <ReviewSettingsMenu settings={settings} onChange={setSettings} />
    </div>
  );
}
