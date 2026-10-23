// The node's live streams (`GET /v1/streams/ws`): text frames (the catalog, the status) and
// binary frames (samples, see imu/frame.ts). The socket reconnects on its own until it is closed.
// `mock-stream.ts` gives the same handlers a simulated robot in `?mock=1`.

import { API_BASE } from "./mode";

export interface StreamHandlers {
  text(text: string): void;
  binary(buffer: ArrayBuffer): void;
  /** The connection dropped (it is retried): nothing arrives until `open` again. */
  closed(): void;
}

/** The WebSocket URL of the live streams on the device helios-api the page came from. */
export function liveStreamUrl(): string {
  const url = new URL("/v1/streams/ws", API_BASE || window.location.origin);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
}

const RETRY_MIN_MS = 1_000;
const RETRY_MAX_MS = 8_000;

/** Opens the live streams. Returns the close function, which stops the retries too. */
export function openLiveStream(handlers: StreamHandlers): () => void {
  let socket: WebSocket | null = null;
  let timer = 0;
  let closed = false;
  let delay = RETRY_MIN_MS;
  const connect = () => {
    timer = 0;
    const next = new WebSocket(liveStreamUrl());
    next.binaryType = "arraybuffer";
    socket = next;
    next.onopen = () => {
      delay = RETRY_MIN_MS;
    };
    next.onmessage = (message: MessageEvent) => {
      if (typeof message.data === "string") handlers.text(message.data);
      else if (message.data instanceof ArrayBuffer) handlers.binary(message.data);
    };
    next.onclose = () => {
      socket = null;
      if (closed) return;
      handlers.closed();
      timer = window.setTimeout(connect, delay);
      delay = Math.min(RETRY_MAX_MS, delay * 2);
    };
  };
  connect();
  return () => {
    closed = true;
    window.clearTimeout(timer);
    socket?.close();
  };
}
