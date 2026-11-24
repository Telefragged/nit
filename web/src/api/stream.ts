// The change-event websocket: the only place the web opens WS /api/stream.
// Components go through openStream (via useChangeStream or useChangeLog): the
// server starts every change the subscription matches with a ChangeProjection
// or its whole log, then sends its entries, and `left` for a change that
// stops matching. When VITE_MOCK is set the fixtures drive it instead of the
// network, mirroring how client.ts routes HTTP.

import { openSocket } from "./socket";
import type { StreamMessage, Subscription } from "./types";

export interface StreamHandle {
  /** Follow the changes the subscription picks; replaces the previous one. */
  subscribe(subscription: Subscription): void;
  close(): void;
}

// The branch is dead code without VITE_MOCK, so production bundles carry no
// fixtures.
const mock = import.meta.env.VITE_MOCK
  ? await import("./fixtures/stream")
  : undefined;

/** `onMessage` receives every `StreamMessage` frame the server writes: a
 * `projection` (a folded ChangeProjection), an `entry` (a log entry), or
 * `left` (a change number). The browser folds them. `positions` returns the
 * `positions` of each sent subscription: the changes that the browser holds,
 * each at its `entries_folded`. */
export function openStream(
  onMessage: (msg: StreamMessage) => void,
  positions: () => Record<number, number>,
): StreamHandle {
  return mock
    ? mock.mockOpenStream(onMessage)
    : openSocketStream(onMessage, positions);
}

/** The real socket. A reconnect sends the subscription again with the
 * current `positions`, so the server sends the entries that the browser
 * lacks. */
function openSocketStream(
  onMessage: (msg: StreamMessage) => void,
  positions: () => Record<number, number>,
): StreamHandle {
  let subscription: Subscription | null = null;
  const send = () => {
    if (subscription === null) return;
    socket.send(JSON.stringify({ ...subscription, positions: positions() }));
  };
  const socket = openSocket("/stream", send, {
    json: (message) => {
      onMessage(message as StreamMessage);
    },
  });
  return {
    subscribe(next) {
      subscription = next;
      send();
    },
    close() {
      socket.close();
    },
  };
}
