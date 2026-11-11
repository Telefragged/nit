import { useState } from "react";
import Seg, { type SegOption } from "./Seg";

const LAYOUTS: SegOption<"unified" | "split">[] = [
  { value: "unified", label: "Unified", title: "One column" },
  { value: "split", label: "Side-by-side", title: "Two columns" },
];

/** The layout choice of the review settings. */
export function Layout() {
  const [value, setValue] = useState<"unified" | "split">("unified");
  return <Seg value={value} options={LAYOUTS} onChange={setValue} />;
}
