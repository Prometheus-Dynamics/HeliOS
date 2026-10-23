import { describe, expect, test } from "bun:test";
import { LiveOutputs, type LiveOutput } from "../src/lib/stores/live-outputs";

const line = (port: string, frame: number, n: number) => JSON.stringify({ pipeline: "front", port, observed_at_ms: 1, frame_ts_ns: frame, value: { n } });

describe("live outputs", () => {
  test("hands over the latest of each output once, however many ticks came", () => {
    const seen: LiveOutput[][] = [];
    const live = new LiveOutputs("ws://x", (outputs) => seen.push(outputs));
    live.ingest([line("detections", 100, 1), line("detections", 200, 2), line("tracks", 200, 3)].join("\n") + "\n");
    live.flush();
    expect(seen.length).toBe(1);
    expect(seen[0].map((o) => [o.id, (o.value.value as { n: number }).n])).toEqual([
      ["front/detections", 2],
      ["front/tracks", 3],
    ]);
    expect(live.received).toBe(3);
  });

  test("finds the output of a frame, or the newest before it", () => {
    const live = new LiveOutputs("ws://x", () => {});
    live.ingest([line("detections", 100, 1), line("detections", 200, 2), line("detections", 300, 3)].join("\n"));
    expect((live.at("front/detections", 200)?.value.value as { n: number }).n).toBe(2);
    expect((live.at("front/detections", 250)?.value.value as { n: number }).n).toBe(2);
    expect((live.at("front/detections", 50)?.value.value as { n: number }).n).toBe(3);
    expect(live.at("front/none", 100)).toBeNull();
  });
});
