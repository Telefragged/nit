import { Toast } from "@base-ui/react/toast";
import { memo, useEffect, useRef, useState } from "react";
import { Kbd } from "./ui/kbd";
import { Toaster, ToastProvider } from "./ui/toast";

const ID = "new-revision";

/** Shows a toast when the latest revision of the change rises above
 * `opened`, the latest revision when the page opened the change. The
 * toast stays until the reviewer closes it or selects the latest
 * revision, and the toast of a later revision replaces it. Its button
 * calls `onOpen` with the latest revision. */
export default memo(function NewRevisionToast({
  opened,
  latest,
  selected,
  onOpen,
}: {
  opened: number;
  latest: number;
  selected: number;
  onOpen: (revision: number) => void;
}) {
  const [manager] = useState(Toast.createToastManager);
  // The open toast keeps the handler of its add, so the button calls the
  // current `onOpen` through this ref.
  const open = useRef(onOpen);
  useEffect(() => {
    open.current = onOpen;
  });
  useEffect(() => {
    if (latest <= opened) return;
    // A toast with the id of an open toast replaces its content.
    manager.add({
      id: ID,
      title: `Revision ${latest} was pushed`,
      timeout: 0,
      actionProps: {
        children: (
          <>
            Open <Kbd aria-hidden>r</Kbd>
          </>
        ),
        onClick: () => {
          open.current(latest);
        },
      },
    });
  }, [manager, opened, latest]);
  // A separate effect, so that a change of `selected` never reopens a
  // toast that the reviewer closed.
  useEffect(() => {
    if (selected === latest) manager.close(ID);
  }, [manager, selected, latest]);
  return (
    <ToastProvider toastManager={manager}>
      <Toaster />
    </ToastProvider>
  );
});
