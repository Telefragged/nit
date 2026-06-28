// The change-event websocket (docs/api.md "Events"): the only place the web
// opens WS /api/stream. Components go through openStream (via useChangeStream)
// in snapshot mode — the server folds a ChangeProj snapshot per change, then
// attaches its live tail. When VITE_MOCK is set the fixtures drive it instead
// of the network, mirroring how client.ts routes HTTP.

import type { ClientMsg, StreamMsg } from "./types";

export interface StreamHandle {
  /** Subscribe to more changes; each yields a snapshot, then its live tail. */
  add(changeIds: number[]): void;
  close(): void;
}

/** `onMessage` receives every `StreamMsg` frame the server writes — a
 * `snapshot` (a folded ChangeProj) or an `entry` (one log entry past it); the
 * browser folds them (docs/api.md "Events"). */
export function openStream(onMessage: (msg: StreamMsg) => void): StreamHandle {
  if (import.meta.env.VITE_MOCK) {
    return openMockStream(onMessage);
  }
  return openSocketStream(onMessage);
}

/** The real socket. Subscribes in snapshot mode; a reconnect (the server closes
 * the socket when a follower overflows) re-subscribes the wanted set, which
 * re-snapshots — the snapshot subsumes a cursor, so none is tracked. */
function openSocketStream(onMessage: (msg: StreamMsg) => void): StreamHandle {
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
    const subscribe_snapshot = [...ids];
    if (subscribe_snapshot.length) {
      ws.send(JSON.stringify({ subscribe_snapshot } satisfies ClientMsg));
    }
  };

  const connect = () => {
    ws = new WebSocket(url());
    ws.onopen = () => {
      backoff = 0;
      subscribe(wanted);
    };
    ws.onmessage = (ev) => {
      let msg: StreamMsg;
      try {
        msg = JSON.parse(ev.data as string) as StreamMsg;
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
    add(changeIds) {
      const fresh = changeIds.filter((id) => !wanted.has(id));
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
function openMockStream(onMessage: (msg: StreamMsg) => void): StreamHandle {
  let mock: StreamHandle | null = null;
  let closed = false;
  const queued: number[] = [];
  void import("./fixtures/stream").then(({ mockOpenStream }) => {
    if (closed) return;
    mock = mockOpenStream(onMessage);
    if (queued.length) mock.add(queued.splice(0));
  });
  return {
    add(changeIds) {
      if (mock) mock.add(changeIds);
      else queued.push(...changeIds);
    },
    close() {
      closed = true;
      mock?.close();
    },
  };
}
