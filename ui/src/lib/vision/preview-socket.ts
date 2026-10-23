// A camera's live preview over its WebSocket (`GET /v1/cameras/{id}/preview/ws`): each frame is a
// binary message (Styx's 32-byte header with the frame's sequence and capture time, then the
// JPEG). Frames are decoded off the main thread; a view draws each with the pipeline outputs of
// the same capture time (the live outputs stream), so overlays follow the video at its own rate.
// A view that falls behind gets the newest frame, not a queue.

export interface PreviewFrame {
  bitmap: ImageBitmap;
  width: number;
  height: number;
  sequence: number;
  timestampNs: number;
}

export interface FrameHeader {
  headerLength: number;
  sequence: number;
  timestampNs: number;
  width: number;
  height: number;
  jpegLength: number;
}

/** Styx's WebSocket frame header (`styx::preview::ws_message`), little-endian. */
export function parseHeader(data: ArrayBuffer): FrameHeader | null {
  if (data.byteLength < 32) return null;
  const view = new DataView(data);
  const headerLength = view.getUint16(4, true);
  if (headerLength < 32 || headerLength > data.byteLength) return null;
  return {
    headerLength,
    sequence: Number(view.getBigUint64(8, true)),
    timestampNs: Number(view.getBigUint64(16, true)),
    width: view.getUint16(24, true),
    height: view.getUint16(26, true),
    jpegLength: view.getUint32(28, true),
  };
}

/** The preview's WebSocket address for its MJPEG address (`…/preview` → `ws(s)://…/preview/ws`). */
export function previewSocketUrl(mjpeg: string, base: string): string {
  const url = new URL(`${mjpeg.replace(/\/$/, "")}/ws`, base);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
}

/**
 * Open the preview: `onframe` gets each decoded frame (close its bitmap when
 * done), `onfail` once if the socket cannot be used (the caller falls back to MJPEG). Returns the
 * close.
 */
export function openPreview(url: string, onframe: (frame: PreviewFrame) => void, onfail: () => void): () => void {
  const socket = new WebSocket(url);
  socket.binaryType = "arraybuffer";
  let closed = false;
  let busy = false;

  socket.onmessage = (event) => {
    if (!(event.data instanceof ArrayBuffer) || busy) return; // Still drawing the last one: skip.
    const header = parseHeader(event.data);
    if (!header) return;
    const jpeg = new Blob([event.data.slice(header.headerLength, header.headerLength + header.jpegLength)], { type: "image/jpeg" });
    busy = true;
    void createImageBitmap(jpeg)
      .catch(() => null)
      .then((bitmap) => {
        busy = false;
        if (!bitmap) return;
        if (closed) return bitmap.close();
        onframe({ bitmap, width: header.width, height: header.height, sequence: header.sequence, timestampNs: header.timestampNs });
      });
  };
  // Failing to open, or dropping later (helios-api restarted): the caller falls back or retries.
  let failed = false;
  const fail = () => {
    if (closed || failed) return;
    failed = true;
    onfail();
  };
  socket.onerror = fail;
  socket.onclose = fail;
  return () => {
    closed = true;
    socket.close();
  };
}
