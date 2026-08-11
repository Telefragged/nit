import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef } from "react";

import { changeDetail, foldEntry } from "../api/fold";
import { openStream, type StreamHandle } from "../api/stream";
import type {
  ChangeDetail,
  ChangeProjection,
  StreamMessage,
} from "../api/types";

/** Keep a set of changes live over the websocket: subscribe in projection mode,
 * hold each change's ChangeProjection, fold its live tail with the shared wasm fold,
 * and write the published projection (revisions/threads/reviews) into the
 * ["change", id] react-query cache. The reviewer's drafts + draft decision are
 * not log state, so they ride a separate ["drafts", id] read (useDrafts); the
 * page composes the two. */
export function useChangeStream(ids: number[]): void {
  const queryClient = useQueryClient();
  // The folded projection per change, mutated in the socket callback (not render).
  const projs = useRef(new Map<number, ChangeProjection>());
  const handle = useRef<StreamHandle | null>(null);

  const publish = useCallback(
    (changeNumber: number) => {
      const proj = projs.current.get(changeNumber);
      if (!proj) return;
      // The wasm projection returns empty drafts/decision — overlaid elsewhere.
      queryClient.setQueryData<ChangeDetail>(
        ["change", changeNumber],
        changeDetail(proj),
      );
    },
    [queryClient],
  );

  useEffect(() => {
    const stream = openStream((msg: StreamMessage) => {
      if ("projection" in msg) {
        projs.current.set(msg.projection.id, msg.projection);
        publish(msg.projection.id);
        return;
      }
      const { change_number } = msg.entry;
      const proj = projs.current.get(change_number);
      // A live entry only ever follows its change's projection.
      if (!proj) return;
      projs.current.set(change_number, foldEntry(proj, msg.entry));
      publish(change_number);
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
