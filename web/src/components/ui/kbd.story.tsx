import { Button } from "./button";
import { Kbd } from "./kbd";

/** A key alone, and the key of a button inside each variant. */
export function Keys() {
  return (
    <div className="flex items-center gap-3">
      <Kbd>c</Kbd>
      <Kbd>shift+n</Kbd>
      <Button variant="primary">
        Submit <Kbd>s</Kbd>
      </Button>
      <Button>
        Review <Kbd>a</Kbd>
      </Button>
      <Button variant="ghost">
        Shortcuts <Kbd>?</Kbd>
      </Button>
    </div>
  );
}
