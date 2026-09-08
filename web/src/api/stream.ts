// The change-event websocket: the only place the web opens WS /api/stream.
// Components go through openStream (via useChangeStream): the server sends a
// ChangeProjection for every change the subscription picks, then their live
// entries. When VITE_MOCK is set the fixtures drive it instead of the
// network, mirroring how client.ts routes HTTP.

import type { StreamMessage, Subscription } from "./types";

export interface StreamHandle {
  /** Follow the changes the subscription picks; replaces the previous one. */
  subscribe(subscription: Subscription): void;
  close(): void;
}

/** `onMessage` receives every `StreamMessage` frame the server writes — a
 * `projection` (a folded ChangeProjection) or an `entry` (one log entry);
 * the browser folds them. */
export function openStream(
  onMessage: (msg: StreamMessage) => void,
): StreamHandle {
  if (import.meta.env.VITE_MOCK) {
    return openMockStream(onMessage);
  }
  return openSocketStream(onMessage);
}

/** The real socket. A reconnect (the server closes the socket when a
 * follower overflows) re-sends the subscription, which re-reads every
 * projection, so no cursor is tracked. */
function openSocketStream(
  onMessage: (msg: StreamMessage) => void,
): StreamHandle {
  let wanted: Subscription | null = null;
  let ws: WebSocket | null = null;
  let closed = false;
  let backoff = 0;

  const url = () => {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${location.host}/api/stream`;
  };

  const send = () => {
    if (ws?.readyState !== WebSocket.OPEN || wanted === null) return;
    ws.send(JSON.stringify(wanted));
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
    subscribe(subscription) {
      wanted = subscription;
      send();
    },
    close() {
      closed = true;
      ws?.close();
    },
  };
}

/** Mock mode: the fixtures replay/emit the stream. Loaded lazily so they stay
 * out of production bundles; `subscribe`/`close` queue until the import
 * resolves. */
function openMockStream(onMessage: (msg: StreamMessage) => void): StreamHandle {
  let mock: StreamHandle | null = null;
  let closed = false;
  let queued: Subscription | null = null;
  void import("./fixtures/stream").then(({ mockOpenStream }) => {
    if (closed) return;
    mock = mockOpenStream(onMessage);
    if (queued !== null) mock.subscribe(queued);
  });
  return {
    subscribe(subscription) {
      if (mock) mock.subscribe(subscription);
      else queued = subscription;
    },
    close() {
      closed = true;
      mock?.close();
    },
  };
}
