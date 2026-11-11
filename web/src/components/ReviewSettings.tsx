import { Settings } from "lucide-react";
import Seg, { type SegOption } from "./Seg";
import { Button } from "./ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTitle,
  PopoverTrigger,
} from "./ui/popover";
import type { ReviewSettings } from "../lib/reviewSettings";

const LAYOUT_OPTIONS: SegOption<ReviewSettings["layout"]>[] = [
  {
    value: "unified",
    label: "Unified",
    title: "One column, old and new interleaved",
  },
  {
    value: "split",
    label: "Side-by-side",
    title: "Two columns, old on the left",
  },
];

const PORTED_OPTIONS: SegOption<ReviewSettings["ported"]>[] = [
  {
    value: "open",
    label: "Open",
    title: "The unresolved threads of earlier revisions",
  },
  {
    value: "all",
    label: "All",
    title: "The resolved threads of earlier revisions too",
  },
];

const WHITESPACE_OPTIONS: SegOption<ReviewSettings["whitespace"]>[] = [
  {
    value: "compare",
    label: "Compare",
    title: "Whitespace makes two lines different",
  },
  {
    value: "ignore",
    label: "Ignore",
    title: "Two lines that differ only in whitespace are one line",
  },
];

/** One knob: a label and a segmented control over its options. */
function Knob<T extends string>({
  label,
  ...seg
}: {
  label: string;
  value: T;
  options: SegOption<T>[];
  onChange: (next: T) => void;
}) {
  return (
    <div className="settings-knob">
      <span>{label}</span>
      <Seg {...seg} />
    </div>
  );
}

/**
 * The diffbar's cog and the popover of settings that it opens. A press on
 * an option calls `onChange` with the changed settings at once, so the
 * diff behind the popover shows the effect.
 */
export default function ReviewSettingsMenu({
  settings,
  onChange,
}: {
  settings: ReviewSettings;
  onChange: (next: ReviewSettings) => void;
}) {
  return (
    <Popover>
      <PopoverTrigger
        render={
          <Button
            variant="ghost"
            size="icon"
            aria-label="Settings"
            title="Review settings"
          />
        }
      >
        <Settings size={15} aria-hidden />
      </PopoverTrigger>
      <PopoverContent className="w-90">
        <div className="flex items-baseline justify-between gap-3">
          <PopoverTitle className="text-base font-semibold">
            Review settings
          </PopoverTitle>
          <span className="dim">Saved in this browser</span>
        </div>
        <Knob
          label="Whitespace"
          value={settings.whitespace}
          options={WHITESPACE_OPTIONS}
          onChange={(whitespace) => {
            onChange({ ...settings, whitespace });
          }}
        />
        <Knob
          label="Columns"
          value={settings.layout}
          options={LAYOUT_OPTIONS}
          onChange={(layout) => {
            onChange({ ...settings, layout });
          }}
        />
        <Knob
          label="Ported comments"
          value={settings.ported}
          options={PORTED_OPTIONS}
          onChange={(ported) => {
            onChange({ ...settings, ported });
          }}
        />
      </PopoverContent>
    </Popover>
  );
}
