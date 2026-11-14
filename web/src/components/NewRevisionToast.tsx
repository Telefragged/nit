import { Toast } from "@base-ui/react/toast";
import { memo, useEffect, useState } from "react";
import { Toaster, ToastProvider } from "./ui/toast";

/** Shows a toast when the latest revision of the change rises above
 * `opened`, the latest revision when the page opened the change. The
 * toast stays until the reviewer closes it, and the toast of a later
 * revision replaces it. */
export default memo(function NewRevisionToast({
  opened,
  latest,
}: {
  opened: number;
  latest: number;
}) {
  const [manager] = useState(Toast.createToastManager);
  useEffect(() => {
    if (latest <= opened) return;
    // A toast with the id of an open toast replaces its content.
    manager.add({
      id: "new-revision",
      title: `Revision ${latest} was pushed`,
      timeout: 0,
    });
  }, [manager, opened, latest]);
  return (
    <ToastProvider toastManager={manager}>
      <Toaster />
    </ToastProvider>
  );
});
