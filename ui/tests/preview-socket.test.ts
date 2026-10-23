import { describe, expect, test } from "bun:test";
import { parseHeader, previewSocketUrl } from "../src/lib/vision/preview-socket";

/** A Styx `ws_message` header (32 bytes, little-endian) followed by `jpeg` bytes. */
function message(sequence: number, timestampNs: number, width: number, height: number, jpeg: number[]): ArrayBuffer {
  const buffer = new ArrayBuffer(32 + jpeg.length);
  const view = new DataView(buffer);
  [0x53, 0x54, 0x59, 0x58].forEach((b, i) => view.setUint8(i, b));
  view.setUint16(4, 32, true);
  view.setUint8(6, 1);
  view.setUint8(7, 0);
  view.setBigUint64(8, BigInt(sequence), true);
  view.setBigUint64(16, BigInt(timestampNs), true);
  view.setUint16(24, width, true);
  view.setUint16(26, height, true);
  view.setUint32(28, jpeg.length, true);
  new Uint8Array(buffer, 32).set(jpeg);
  return buffer;
}

describe("preview socket", () => {
  test("reads Styx's frame header", () => {
    const header = parseHeader(message(42, 1_000_000_123, 640, 400, [0xff, 0xd8, 0xff, 0xd9]));
    expect(header).toEqual({ headerLength: 32, sequence: 42, timestampNs: 1_000_000_123, width: 640, height: 400, jpegLength: 4 });
    expect(parseHeader(new ArrayBuffer(10))).toBeNull();
  });

  test("turns the MJPEG address into the socket's", () => {
    expect(previewSocketUrl("/v1/cameras/front/preview", "http://helios.local:5800/")).toBe("ws://helios.local:5800/v1/cameras/front/preview/ws");
    expect(previewSocketUrl("https://robot/v1/cameras/a/preview", "https://robot/")).toBe("wss://robot/v1/cameras/a/preview/ws");
  });
});
