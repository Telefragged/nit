import type { Run } from "../../api/document.gen";
import { ago } from "../../api/fixtures/builders";
import VersionList from "./VersionList";

const RUNS: Run[] = [
  { writer: "author", version: "212@1", at: ago(180) },
  { writer: "reviewer", version: "264@2", at: ago(90) },
  { writer: "author", version: "301@3", at: ago(45) },
  { writer: "reviewer", version: "318@2", at: ago(2) },
];

const list = (selected: number | null) => (
  <div className="document-page" style={{ width: 720 }}>
    <VersionList
      runs={RUNS}
      sent="264@2"
      selected={selected}
      onSelect={() => undefined}
    />
  </div>
);

export const Default = () => list(null);

export const Selected = () => list(2);
