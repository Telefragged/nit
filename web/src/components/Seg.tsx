/** One choice of a segmented control. */
export interface SegOption<T> {
  value: T;
  label: string;
  title: string;
}

/** A segmented control: one button per option, the chosen one active. */
export default function Seg<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: readonly SegOption<T>[];
  onChange: (next: T) => void;
}) {
  return (
    <span className="seg">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
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
  );
}
