import Select, { type SelectOption } from "./Select";

/** The two ends of a diff over `items`, which are in order: the left
 * picker selects the base of the diff, and the right picker selects the
 * item that the diff ends at.
 *
 * `option` gives the label and the detail of an item. `selected` is
 * the index of the item of the right picker, and `onRight` receives the
 * index of a pick there. The left picker offers `first`, the base
 * before every item, and then each item with `baseValue` of it as its
 * value. An item at or after `selected` is offered but not choosable.
 * `against` is the value of the left picker, and `onLeft` receives the
 * value of a pick there. */
const FITS_ITEM_AND_DETAIL = "min-w-[18ch]";

export default function DiffRangeSelect<T>({
  items,
  option,
  baseValue,
  first,
  selected,
  against,
  baseTitle,
  label,
  title,
  placeholder,
  onLeft,
  onRight,
}: {
  items: readonly T[];
  option: (item: T, index: number) => Pick<SelectOption, "label" | "detail">;
  baseValue: (item: T, index: number) => string;
  first: SelectOption;
  selected: number;
  against: string;
  baseTitle: string;
  /** The label, the title and the placeholder of the right picker. */
  label: string;
  title: string;
  placeholder: string;
  onLeft: (value: string) => void;
  onRight: (index: number) => void;
}) {
  return (
    <>
      <Select
        className={FITS_ITEM_AND_DETAIL}
        label="Diff base"
        title={baseTitle}
        placeholder="no base"
        value={against}
        onChange={onLeft}
        options={[
          first,
          ...items.map((item, index) => ({
            ...option(item, index),
            value: baseValue(item, index),
            disabled: index >= selected,
          })),
        ]}
      />
      <span className="dim mono">→</span>
      <Select
        className="min-w-[18ch]"
        label={label}
        title={title}
        placeholder={placeholder}
        value={String(selected)}
        onChange={(value) => {
          onRight(Number(value));
        }}
        options={items.map((item, index) => ({
          ...option(item, index),
          value: String(index),
        }))}
      />
    </>
  );
}
