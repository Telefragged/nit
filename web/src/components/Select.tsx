import {
  Select as SelectRoot,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";

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
 * `className` goes to the head, so that a caller can size it.
 */
export default function Select({
  options,
  value,
  onChange,
  label,
  title,
  placeholder,
  className,
}: {
  options: SelectOption[];
  value: string;
  onChange: (value: string) => void;
  /** Names the picker for assistive tech. */
  label: string;
  title?: string;
  placeholder: string;
  className?: string;
}) {
  const selected = options.find((option) => option.value === value);
  return (
    <SelectRoot
      value={selected ? value : null}
      onValueChange={(next: string | null) => {
        if (next !== null) onChange(next);
      }}
      disabled={options.length === 0}
    >
      <SelectTrigger className={className} aria-label={label} title={title}>
        <SelectValue>
          {() => (selected ? <Row option={selected} /> : placeholder)}
        </SelectValue>
      </SelectTrigger>
      <SelectContent
        // The page has one-key shortcuts. A key press in the open list
        // must not also trigger them.
        onKeyDown={(e) => {
          e.stopPropagation();
        }}
      >
        {options.map((option) => (
          <SelectItem
            key={option.value}
            value={option.value}
            disabled={option.disabled}
          >
            <Row option={option} />
          </SelectItem>
        ))}
      </SelectContent>
    </SelectRoot>
  );
}

function Row({ option }: { option: SelectOption }) {
  return (
    <>
      <span className="truncate">{option.label}</span>
      {option.detail === undefined ? null : (
        // The space keeps the two apart in the option's accessible name.
        <>
          {" "}
          <span className="ml-auto truncate text-faint-foreground">
            {option.detail}
          </span>
        </>
      )}
    </>
  );
}
