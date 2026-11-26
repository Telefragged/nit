import { revisionVersion } from "../lib/version";
import Select from "./Select";

const FITS_ITEM_AND_DETAIL = "min-w-[18ch]";

/** The two ends of the diff of a change: the left picker selects the
 * base of the diff, and the right picker selects the revision under
 * review.
 *
 * `details` contains the detail of each revision, by its number.
 * `selected` is the revision of the right picker, and `onRight` receives
 * the revision of a pick there. The left picker offers the base of the
 * change, as `"base"`, and then each revision as its version. A revision
 * at or after `selected` is offered but not choosable. `against` is the
 * value of the left picker, and `onLeft` receives the value of a pick
 * there. */
export default function DiffRangeSelect({
  details,
  selected,
  against,
  onLeft,
  onRight,
}: {
  details: readonly (string | undefined)[];
  selected: number;
  against: string;
  onLeft: (value: string) => void;
  onRight: (revision: number) => void;
}) {
  const revisions = details.map((detail, number) => ({
    label: `r${number}`,
    detail,
  }));
  return (
    <>
      <Select
        className={FITS_ITEM_AND_DETAIL}
        label="Diff base"
        title="Base = parent commit; rM = interdiff against revision M"
        placeholder="no base"
        value={against}
        onChange={onLeft}
        options={[
          { value: "base", label: "Base" },
          ...revisions.map((revision, number) => ({
            ...revision,
            value: revisionVersion(number),
            disabled: number >= selected,
          })),
        ]}
      />
      <span className="dim mono">→</span>
      <Select
        className={FITS_ITEM_AND_DETAIL}
        label="Revision"
        title="Revision under review"
        placeholder="no revisions"
        value={String(selected)}
        onChange={(value) => {
          onRight(Number(value));
        }}
        options={revisions.map((revision, number) => ({
          ...revision,
          value: String(number),
        }))}
      />
    </>
  );
}
