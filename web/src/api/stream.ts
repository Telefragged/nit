// The change-event websocket: the only place the web opens WS /api/stream.
// Components go through openStream (via useChangeStream)
// in projection mode — the server folds a ChangeProjection per change, then
// attaches its live tail. When VITE_MOCK is set the fixtures drive it instead
// of the network, mirroring how client.ts routes HTTP.

import type { ClientMessage, StreamMessage } from "./types";

export interface StreamHandle {
  /** Subscribe to more changes; each yields a projection, then its live tail. */
  add(changeNumbers: number[]): void;
  close(): void;
}

/** `onMessage` receives every `StreamMessage` frame the server writes — a
 * `projection` (a folded ChangeProjection) or an `entry` (one log entry past it); the
 * browser folds them. */
export function openStream(
  onMessage: (msg: StreamMessage) => void,
): StreamHandle {
  if (import.meta.env.VITE_MOCK) {
    return openMockStream(onMessage);
  }
  return openSocketStream(onMessage);
}

/** The real socket. Subscribes in projection mode; a reconnect (the server closes
 * the socket when a follower overflows) re-subscribes the wanted set, which
 * re-reads the projection, which subsumes a cursor, so none is tracked. */
function openSocketStream(
  onMessage: (msg: StreamMessage) => void,
): StreamHandle {
  const wanted = new Set<number>();
  let ws: WebSocket | null = null;
  let closed = false;
  let backoff = 0;

  const url = () => {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${location.host}/api/stream`;
  };

  const subscribe = (ids: Iterable<number>) => {
    if (ws?.readyState !== WebSocket.OPEN) return;
    const subscribe_projection = [...ids];
    if (subscribe_projection.length) {
      ws.send(JSON.stringify({ subscribe_projection } satisfies ClientMessage));
    }
  };

  const connect = () => {
    ws = new WebSocket(url());
    ws.onopen = () => {
      backoff = 0;
      subscribe(wanted);
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
    add(changeNumbers) {
      const fresh = changeNumbers.filter((id) => !wanted.has(id));
      for (const id of fresh) wanted.add(id);
      subscribe(fresh);
    },
    close() {
      closed = true;
      ws?.close();
    },
  };
}

/** Mock mode: the fixtures replay/emit the stream. Loaded lazily so they stay
 * out of production bundles; `add`/`close` queue until the import resolves. */
function openMockStream(onMessage: (msg: StreamMessage) => void): StreamHandle {
  let mock: StreamHandle | null = null;
  let closed = false;
  const queued: number[] = [];
  void import("./fixtures/stream").then(({ mockOpenStream }) => {
    if (closed) return;
    mock = mockOpenStream(onMessage);
    if (queued.length) mock.add(queued.splice(0));
  });
  return {
    add(changeNumbers) {
      if (mock) mock.add(changeNumbers);
      else queued.push(...changeNumbers);
    },
    close() {
      closed = true;
      mock?.close();
    },
  };
}
