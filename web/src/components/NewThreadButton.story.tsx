import NewThreadButton from "./NewThreadButton";

/** The button of each anchor that a new thread can have without a line. */
export function Anchors() {
  return (
    <div style={{ display: "flex", gap: 20 }}>
      <NewThreadButton anchor="change" onClick={() => undefined} />
      <NewThreadButton anchor="file" onClick={() => undefined} />
    </div>
  );
}
