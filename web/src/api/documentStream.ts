import type {
  DocumentNumber,
  DocumentSubscription,
  Version,
} from "./document.gen";
import { openSocket } from "./socket";

// The branch is dead code without VITE_MOCK, so production bundles contain
// no fixtures.
const mock = import.meta.env.VITE_MOCK
  ? await import("./fixtures/documents")
  : undefined;

const DOCUMENT_NUMBER_BYTES = 8;

export function decodeUpdateFrame(frame: Uint8Array): {
  document: DocumentNumber;
  update: Uint8Array;
} {
  const number = new DataView(frame.buffer, frame.byteOffset).getBigUint64(
    0,
    true,
  );
  return {
    document: Number(number),
    update: frame.subarray(DOCUMENT_NUMBER_BYTES),
  };
}

/** Follows the edits of document `number`. `onUpdate` receives the bytes
 * of each update that the server has and a replica at `version()` lacks.
 * A reconnect sends the current `version()` again. Returns the function
 * that stops the stream. */
export function followDocument(
  number: DocumentNumber,
  version: () => Version,
  onUpdate: (update: Uint8Array) => void,
): () => void {
  if (mock) return mock.mockFollowDocument(number, version(), onUpdate);
  const socket = openSocket(
    "/documents/stream",
    () => {
      const subscription: DocumentSubscription = {
        updates: { [number]: version() },
        sent: null,
        seen: {},
      };
      socket.send(JSON.stringify(subscription));
    },
    {
      bytes: (frame) => {
        const { document, update } = decodeUpdateFrame(frame);
        if (document === number) onUpdate(update);
      },
    },
  );
  return () => {
    socket.close();
  };
}
