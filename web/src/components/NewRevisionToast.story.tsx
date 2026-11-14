import NewRevisionToast from "./NewRevisionToast";

/** The toast after a push of r2 to a change that the page opened
 * at r1. The toast sits in the corner of the viewport, so the story is as
 * large as the viewport. */
export function Pushed() {
  return (
    <div className="h-screen w-screen">
      <NewRevisionToast
        opened={1}
        latest={2}
        selected={1}
        onOpen={() => undefined}
      />
    </div>
  );
}
