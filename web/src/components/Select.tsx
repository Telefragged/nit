import { useEffect, useId, useRef, useState } from "react";

/** One row of a `Select`: `label` names it, `detail` qualifies it. */
export interface SelectOption {
  value: string;
  label: string;
  detail?: string;
  /** Offered but not choosable, as a native `<option disabled>` is. */
  disabled?: boolean;
}

/**
 * A picker over `options`: each row carries its label and, right-aligned
 * and dim, its detail, so the reader takes the label first and the
 * qualifier second.
 *
 * Choosing a row calls `onChange` with its value and closes the list, as
 * do Escape and a press outside the picker. `placeholder` stands in when
 * `value` names no option; with no options at all the head disables
 * itself.
 *
 * A native `<select>` cannot lay an option out in two columns, so this is
 * a button over a list of buttons. The list drops below the head, which
 * stays put, so a second press on the head closes it again.
 */
export default function Select({
  options,
  value,
  onChange,
  label,
  title,
  placeholder,
}: {
  options: SelectOption[];
  value: string;
  onChange: (value: string) => void;
  /** Names the picker for assistive tech. */
  label: string;
  title?: string;
  placeholder: string;
}) {
  const [open, setOpen] = useState(false);
  const listId = useId();
  const group = useRef<HTMLDivElement>(null);
  const head = useRef<HTMLButtonElement>(null);
  const selected = options.find((option) => option.value === value);

  // The head takes focus back from the row that unmounts under it, so the
  // next tab carries on from the picker instead of the top of the page.
  const close = () => {
    setOpen(false);
    head.current?.focus();
  };

  // An open list owns the pointer and the keyboard, so both listeners sit
  // on the document: the press that dismisses it lands anywhere on the
  // page, and the page's one-key shortcuts must not fire underneath it.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      if (!group.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
      e.stopPropagation();
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [open]);

  return (
    <div className="select" ref={group}>
      <button
        className="select-head"
        ref={head}
        role="combobox"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        title={title}
        disabled={options.length === 0}
        onClick={() => {
          setOpen((v) => !v);
        }}
      >
        {selected === undefined ? (
          <span className="select-label">{placeholder}</span>
        ) : (
          <Row option={selected} />
        )}
        <svg className="select-caret" viewBox="0 0 10 6" aria-hidden="true">
          <path
            d="M1 1 5 5 9 1"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
          />
        </svg>
      </button>
      {open ? (
        <div className="select-list" id={listId} role="listbox">
          {options.map((option) => (
            <button
              key={option.value}
              role="option"
              aria-selected={option.value === value}
              disabled={option.disabled}
              onClick={() => {
                onChange(option.value);
                close();
              }}
            >
              <Row option={option} />
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function Row({ option }: { option: SelectOption }) {
  return (
    <>
      <span className="select-label">{option.label}</span>
      {option.detail === undefined ? null : (
        // The space keeps the two apart in the option's accessible name.
        <>
          {" "}
          <span className="select-detail">{option.detail}</span>
        </>
      )}
    </>
  );
}
