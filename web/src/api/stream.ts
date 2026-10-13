// The change-event websocket: the only place the web opens WS /api/stream.
// Components go through openStream (via useChangeStream): the server sends a
// ChangeProjection for every change the subscription matches, then their
// entries, and `left` for a change that stops matching. When VITE_MOCK is set
// the fixtures drive it instead of the network, mirroring how client.ts routes
// HTTP.

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
  let ws: WebSocket | null = null;
  let closed = false;
  let backoff = 0;

  const url = () => {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${location.host}/api/stream`;
  };

  const send = () => {
    if (ws?.readyState !== WebSocket.OPEN || subscription === null) return;
    ws.send(JSON.stringify({ ...subscription, positions: positions() }));
  };

  const connect = () => {
    ws = new WebSocket(url());
    ws.onopen = () => {
      backoff = 0;
      send();
    };
    ws.onmessage = (ev) => {
      let msg: StreamMessage;
      try {
        msg = JSON.parse(ev.data as string) as StreamMessage;
      } catch {
        return;
      }
      onMessage(msg);
    };
    ws.onclose = () => {
      if (closed) return;
      const delay = Math.min(500 * 2 ** backoff++, 10_000);
      setTimeout(connect, delay);
    };
    ws.onerror = () => {
      ws?.close();
    };
  };
  connect();

  return {
    subscribe(next) {
      subscription = next;
      send();
    },
    close() {
      closed = true;
      ws?.close();
    },
  };
}
