import ReviewColumns from "./ReviewColumns";

/** A rail and a diff column with a line of text each. */
export function Columns() {
  return (
    <div className="w-[900px] border border-border font-mono text-xs">
      <ReviewColumns rail="crates/nit/src/server.rs">
        <div className="h-40 p-3 text-muted-foreground">
          The diff of crates/nit/src/server.rs
        </div>
      </ReviewColumns>
    </div>
  );
}
