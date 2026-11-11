import { Settings } from "lucide-react";
import { useState } from "react";
import { confirmDiscard } from "../lib/confirmDiscard";
import Modal from "./Modal";
import Seg, { type SegOption } from "./Seg";
import { Button } from "./ui/button";
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
 * The diffbar's cog and the settings modal it opens. The modal edits a
 * draft copy: **Apply** hands it to `onApply`, which saves it, and every
 * other exit throws it away.
 */
export default function ReviewSettingsMenu({
  settings,
  onApply,
}: {
  settings: ReviewSettings;
  onApply: (next: ReviewSettings) => void;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(settings);

  // Every knob compares itself, so a knob added later needs no edit here.
  const edited = (Object.keys(draft) as (keyof ReviewSettings)[]).some(
    (knob) => draft[knob] !== settings[knob],
  );

  // A backdrop press can be a mis-click, so it asks before dropping edits.
  // Cancel and Escape say the same thing on purpose, and just close.
  const requestClose = () => {
    if (!confirmDiscard(edited, "settings")) return;
    setOpen(false);
  };

  return (
    <>
      <Button
        variant="ghost"
        size="icon"
        aria-label="Settings"
        title="Review settings"
        onClick={() => {
          setDraft(settings);
          setOpen(true);
        }}
      >
        <Settings size={15} aria-hidden />
      </Button>
      {open ? (
        <Modal
          title="Review settings"
          aside={<span className="dim">Saved in this browser</span>}
          card="settings-modal"
          onDismiss={(via) => {
            if (via === "backdrop") requestClose();
            else setOpen(false);
          }}
          actions={
            <>
              <Button
                variant="ghost"
                onClick={() => {
                  setOpen(false);
                }}
              >
                Cancel
              </Button>
              <span className="spacer" />
              <Button
                onClick={() => {
                  onApply(draft);
                  setOpen(false);
                }}
              >
                Apply
              </Button>
            </>
          }
        >
          <Knob
            label="Whitespace"
            value={draft.whitespace}
            options={WHITESPACE_OPTIONS}
            onChange={(whitespace) => {
              setDraft({ ...draft, whitespace });
            }}
          />
          <Knob
            label="Columns"
            value={draft.layout}
            options={LAYOUT_OPTIONS}
            onChange={(layout) => {
              setDraft({ ...draft, layout });
            }}
          />
          <Knob
            label="Ported comments"
            value={draft.ported}
            options={PORTED_OPTIONS}
            onChange={(ported) => {
              setDraft({ ...draft, ported });
            }}
          />
        </Modal>
      ) : null}
    </>
  );
}
