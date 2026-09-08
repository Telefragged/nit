import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";

import { changeDetail, foldEntry } from "../api/fold";
import { openStream, type StreamHandle } from "../api/stream";
import type {
  ChangeDetail,
  ChangeProjection,
  StreamMessage,
  Subscription,
} from "../api/types";

/** Keep the changes a subscription picks live over the websocket: hold each
 * one's ChangeProjection, fold its live tail with the shared wasm fold, and
 * write the published projection (revisions/threads/reviews) into the
 * ["change", id] react-query cache. Returns the picked change numbers,
 * ascending, as their projections arrive. The reviewer's drafts + draft
 * decision are not log state, so they ride a separate ["drafts", id] read
 * (useDrafts); the page composes the two. */
export function useChangeStream(
  subscription: Subscription | undefined,
): number[] {
  const queryClient = useQueryClient();
  // The folded projection per change, mutated in the socket callback.
  const projs = useRef(new Map<number, ChangeProjection>());
  const handle = useRef<StreamHandle | null>(null);
  const [ids, setIds] = useState<number[]>([]);

  useEffect(() => {
    const publish = (changeNumber: number) => {
      const proj = projs.current.get(changeNumber);
      if (!proj) return;
      // The wasm projection returns empty drafts/decision — overlaid elsewhere.
      queryClient.setQueryData<ChangeDetail>(
        ["change", changeNumber],
        changeDetail(proj),
      );
    };
    const stream = openStream((msg: StreamMessage) => {
      if ("projection" in msg) {
        const id = msg.projection.id;
        projs.current.set(id, msg.projection);
        publish(id);
        setIds((prev) =>
          prev.includes(id) ? prev : [...prev, id].sort((a, b) => a - b),
        );
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
  }, [queryClient]);

  // A new subscription replaces the old one on the same socket; its
  // projections start the picked set over. The reset is adjust-during-render
  // so the old ids never render against the new subscription.
  const key = subscription === undefined ? null : JSON.stringify(subscription);
  const [sentKey, setSentKey] = useState<string | null>(null);
  if (sentKey !== key) {
    setSentKey(key);
    setIds([]);
  }
  useEffect(() => {
    if (key === null) return;
    projs.current.clear();
    handle.current?.subscribe(JSON.parse(key) as Subscription);
  }, [key]);

  return ids;
}
