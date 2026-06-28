import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef } from "react";

import { changeDetail, foldEntry } from "../api/fold";
import { openStream, type StreamHandle } from "../api/stream";
import type { ChangeDetail, ChangeProj, StreamMsg } from "../api/types";

/** Keep a set of changes live over the websocket: subscribe in snapshot mode,
 * hold each change's ChangeProj, fold its live tail with the shared wasm fold,
 * and write the published projection (revisions/threads/reviews) into the
 * ["change", id] react-query cache. The reviewer's drafts + staged decision are
 * not log state, so they ride a separate ["drafts", id] read (useDrafts); the
 * page composes the two. */
export function useChangeStream(ids: number[]): void {
  const queryClient = useQueryClient();
  // The folded projection per change, mutated in the socket callback (not render).
  const projs = useRef(new Map<number, ChangeProj>());
  const handle = useRef<StreamHandle | null>(null);

  const publish = useCallback(
    (changeId: number) => {
      const proj = projs.current.get(changeId);
      if (!proj) return;
      // The wasm projection returns empty drafts/decision — overlaid elsewhere.
      queryClient.setQueryData<ChangeDetail>(
        ["change", changeId],
        changeDetail(proj),
      );
    },
    [queryClient],
  );

  useEffect(() => {
    const stream = openStream((msg: StreamMsg) => {
      if ("snapshot" in msg) {
        projs.current.set(msg.snapshot.id, msg.snapshot);
        publish(msg.snapshot.id);
        return;
      }
      const { change_id } = msg.entry;
      const proj = projs.current.get(change_id);
      // A live entry only ever follows its change's snapshot.
      if (!proj) return;
      projs.current.set(change_id, foldEntry(proj, msg.entry));
      publish(change_id);
    });
    handle.current = stream;
    return () => {
      stream.close();
    };
  }, [publish]);

  const key = ids.join(",");
  useEffect(() => {
    handle.current?.add(key.split(",").filter(Boolean).map(Number));
  }, [key]);
}
