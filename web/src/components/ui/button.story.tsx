import { ChevronDown } from "lucide-react";
import { Button } from "./button";

/** Each variant and size, enabled and disabled. */
export function Variants() {
  return (
    <div className="grid grid-cols-[repeat(6,max-content)] items-center gap-3">
      <Button variant="primary">Submit</Button>
      <Button variant="primary" disabled>
        Submit
      </Button>
      <Button>Review</Button>
      <Button disabled>Review</Button>
      <Button variant="ghost">Reply</Button>
      <Button variant="ghost" disabled>
        Reply
      </Button>
      <Button variant="primary" size="sm">
        Submit
      </Button>
      <Button variant="primary" size="sm" disabled>
        Submit
      </Button>
      <Button size="sm">Resolve</Button>
      <Button size="sm" disabled>
        Resolve
      </Button>
      <Button variant="ghost" size="sm">
        Reply
      </Button>
      <Button variant="ghost" size="icon" aria-label="Collapse thread">
        <ChevronDown size={14} aria-hidden />
      </Button>
    </div>
  );
}
