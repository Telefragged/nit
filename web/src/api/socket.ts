/** A websocket of the nit API that connects again after it closes. */
export interface Socket {
  /** Sends a text frame. An unconnected socket drops it. */
  send(text: string): void;
  close(): void;
}

/** Opens the websocket at `path` under `/api`. `onOpen` runs after each
 * connect, so the caller sends its subscription again. `on.json` receives
 * the JSON value of each text frame, and `on.bytes` receives each binary
 * frame. */
export function openSocket(
  path: string,
  onOpen: () => void,
  on: {
    json?: (message: unknown) => void;
    bytes?: (frame: Uint8Array) => void;
  },
): Socket {
  let ws: WebSocket | null = null;
  let closed = false;
  let backoff = 0;

  const connect = () => {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    ws = new WebSocket(`${proto}//${location.host}/api${path}`);
    ws.binaryType = "arraybuffer";
    ws.onopen = () => {
      backoff = 0;
      onOpen();
    };
    ws.onmessage = (ev: MessageEvent<string | ArrayBuffer>) => {
      if (typeof ev.data !== "string") {
        on.bytes?.(new Uint8Array(ev.data));
        return;
      }
      let message: unknown;
      try {
        message = JSON.parse(ev.data);
      } catch {
        return;
      }
      on.json?.(message);
    };
    ws.onclose = () => {
      if (closed) return;
      // No event reports that the server is back, so the connect repeats
      // on a timer.
      const delay = Math.min(500 * 2 ** backoff++, 10_000);
      setTimeout(connect, delay);
    };
    ws.onerror = () => {
      ws?.close();
    };
  };
  connect();

  return {
    send(text) {
      if (ws?.readyState === WebSocket.OPEN) ws.send(text);
    },
    close() {
      closed = true;
      ws?.close();
    },
  };
}
