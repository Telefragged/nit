import { Button } from "./button";

/** Each variant, enabled and disabled. */
export function Variants() {
  return (
    <div className="flex items-center gap-3">
      <Button variant="primary">Submit</Button>
      <Button variant="primary" disabled>
        Submit
      </Button>
      <Button>Review</Button>
      <Button disabled>Review</Button>
    </div>
  );
}
