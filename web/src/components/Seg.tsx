import { ToggleGroup, ToggleGroupItem } from "./ui/toggle-group";

/** One choice of a segmented control. */
export interface SegOption<T> {
  value: T;
  label: string;
  title: string;
}

/** A segmented control: one toggle per option, the chosen one pressed.
 * A press on the chosen option does not change the choice. */
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
    <ToggleGroup
      value={[value]}
      onValueChange={([next]) => {
        if (next !== undefined) onChange(next);
      }}
    >
      {options.map((option) => (
        <ToggleGroupItem
          key={option.value}
          value={option.value}
          title={option.title}
        >
          {option.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
