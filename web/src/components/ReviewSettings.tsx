import { useState } from "react";
import { confirmDiscard } from "../lib/confirmDiscard";
import Modal from "./Modal";
import type { ReviewSettings } from "../lib/reviewSettings";

interface Option<T> {
  value: T;
  label: string;
  title: string;
}

const MODE_OPTIONS: Option<ReviewSettings["mode"]>[] = [
  {
    value: "full",
    label: "Full",
    title: "Every line the change touched (0)",
  },
  {
    value: "outline",
    label: "Outline",
    title: "Every function body collapsed, leaving signatures (0)",
  },
];

const LAYOUT_OPTIONS: Option<ReviewSettings["layout"]>[] = [
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

/** One knob: a label and a segmented control over its options. */
function Knob<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: Option<T>[];
  onChange: (next: T) => void;
}) {
  return (
    <div className="settings-knob">
      <span>{label}</span>
      <span className="seg">
        {options.map((option) => (
          <button
            key={option.value}
            className={value === option.value ? "active" : ""}
            title={option.title}
            onClick={() => {
              onChange(option.value);
            }}
          >
            {option.label}
          </button>
        ))}
      </span>
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
      <button
        className="settings-cog"
        aria-label="Settings"
        title="Review settings"
        onClick={() => {
          setDraft(settings);
          setOpen(true);
        }}
      >
        ⚙
      </button>
      {open ? (
        <Modal
          label="Settings"
          card="settings-modal"
          onDismiss={(via) => {
            if (via === "backdrop") requestClose();
            else setOpen(false);
          }}
        >
          <div className="modal-head">
            <strong>Review settings</strong>
            <span className="dim">Saved in this browser</span>
          </div>
          <Knob
            label="Diff body"
            value={draft.mode}
            options={MODE_OPTIONS}
            onChange={(mode) => {
              setDraft({ ...draft, mode });
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
          <div className="modal-actions">
            <button
              onClick={() => {
                setOpen(false);
              }}
            >
              Cancel
            </button>
            <span className="spacer" />
            <button
              className="btn-primary"
              onClick={() => {
                onApply(draft);
                setOpen(false);
              }}
            >
              Apply
            </button>
          </div>
        </Modal>
      ) : null}
    </>
  );
}
