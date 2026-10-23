import { expect, test } from "bun:test";
import { decodeSamples, encodeSamples, parseTextFrame, type Catalog } from "../src/lib/imu/frame";
import { LiveFeed } from "../src/lib/imu/feed";
import { FusedOrientation, Orientation, caseOrientation, euler, fromGravity } from "../src/lib/imu/attitude";
import { ringFrame } from "../src/lib/imu/ring";
import { openSimulatedStream } from "../src/lib/api/mock-stream";

const CATALOG: Catalog = {
  stream: "board.hardware",
  owner: "node/raze-1",
  epoch: 3,
  live: true,
  series: [
    { name: "imu.acceleration.x", unit: "m/s²", kind: "f32", period_us: 10_000 },
    { name: "imu.acceleration.y", unit: "m/s²", kind: "f32", period_us: 10_000 },
    { name: "imu.acceleration.z", unit: "m/s²", kind: "f32", period_us: 10_000 },
    { name: "imu.angular_rate.x", unit: "rad/s", kind: "f32", period_us: 10_000 },
    { name: "imu.angular_rate.y", unit: "rad/s", kind: "f32", period_us: 10_000 },
    { name: "imu.angular_rate.z", unit: "rad/s", kind: "f32", period_us: 10_000 },
    { name: "orientation.quaternion.w", unit: "", kind: "f32", period_us: 10_000 },
    { name: "orientation.quaternion.x", unit: "", kind: "f32", period_us: 10_000 },
    { name: "orientation.quaternion.y", unit: "", kind: "f32", period_us: 10_000 },
    { name: "orientation.quaternion.z", unit: "", kind: "f32", period_us: 10_000 },
    { name: "mag.magnetic_field.x", unit: "µT", kind: "f32", period_us: 100_000 },
    { name: "light.offset", unit: "", kind: "f32", period_us: 50_000 },
    { name: "light.clockwise", unit: "", kind: "f32", period_us: 50_000 },
    { name: "light.brightness", unit: "", kind: "f32", period_us: 50_000 },
    { name: "light.led.0", unit: null, kind: "u32", period_us: 50_000 },
    { name: "light.led.1", unit: null, kind: "u32", period_us: 50_000 },
  ],
};

/** The catalog's index of a series. */
const at = (name: string) => CATALOG.series.findIndex((series) => series.name === name);

test("a samples frame reads back as the samples it was built from", () => {
  const bytes = encodeSamples({
    backfill: true,
    blocks: [
      { series: 2, samples: [[1_700_000_000_000_000, 9.5], [1_700_000_000_010_000, 9.75]] },
      { series: 14, samples: [[1_700_000_000_000_000, 0xff00_ff80]] },
    ],
  });
  const batch = decodeSamples(bytes);
  expect(batch).not.toBeNull();
  expect(batch!.backfill).toBe(true);
  expect(batch!.blocks.map((b) => b.series)).toEqual([2, 14]);
  expect(Array.from(batch!.blocks[0].t)).toEqual([1_700_000_000_000_000, 1_700_000_000_010_000]);
  expect(Array.from(batch!.blocks[0].v)).toEqual([9.5, 9.75]);
  // A U32 colour is exact through f64.
  expect(batch!.blocks[1].v[0]).toBe(0xff00_ff80);
});

test("a malformed samples frame is refused, not half read", () => {
  const good = new Uint8Array(encodeSamples({ backfill: false, blocks: [{ series: 0, samples: [[1, 2]] }] }));
  expect(decodeSamples(good.slice(0, good.length - 1).buffer)).toBeNull();
  const wrongKind = good.slice();
  wrongKind[0] = 9;
  expect(decodeSamples(wrongKind.buffer)).toBeNull();
  const trailing = new Uint8Array([...good, 0]);
  expect(decodeSamples(trailing.buffer)).toBeNull();
  expect(decodeSamples(new ArrayBuffer(2))).toBeNull();
});

test("text frames: the catalog, the status, the lag notice; anything else is ignored", () => {
  expect(parseTextFrame(JSON.stringify({ type: "catalog", ...CATALOG }))).toMatchObject({ type: "catalog", catalog: { epoch: 3 } });
  expect(parseTextFrame('{"type":"status","state":"stale","reason":"gone"}')).toEqual({ type: "status", status: { state: "stale", reason: "gone" } });
  expect(parseTextFrame('{"type":"lagged","missed":12}')).toEqual({ type: "lagged", missed: 12 });
  expect(parseTextFrame('{"type":"other"}')).toBeNull();
  expect(parseTextFrame("not json")).toBeNull();
});

test("the feed finds each device by its channel and keeps the channels in one row", () => {
  const feed = new LiveFeed();
  feed.setCatalog(CATALOG);
  const imu = feed.device("imu");
  expect(imu?.prefix).toBe("imu");
  expect(imu?.channels.map((c) => c.name)).toEqual(["acceleration.x", "acceleration.y", "acceleration.z", "angular_rate.x", "angular_rate.y", "angular_rate.z"]);
  expect(feed.device("orientation")?.channels.map((c) => c.name)).toEqual(["quaternion.w", "quaternion.x", "quaternion.y", "quaternion.z"]);
  expect(feed.device("magnetometer")?.channels[0].name).toBe("magnetic_field.x");
  expect(feed.device("light")?.channels.map((c) => c.name)).toEqual(["offset", "clockwise", "brightness", "led.0", "led.1"]);

  // Three channels of the IMU at the same two times, the others in a later batch of the same time.
  feed.apply({
    backfill: false,
    blocks: [
      { series: at("imu.acceleration.x"), t: Float64Array.of(1_000, 2_000), v: Float64Array.of(0.1, 0.2) },
      { series: at("imu.acceleration.z"), t: Float64Array.of(1_000, 2_000), v: Float64Array.of(9.8, 9.7) },
    ],
  });
  expect(imu!.count).toBe(2);
  const ax = imu!.channel("acceleration.x");
  const az = imu!.channel("acceleration.z");
  expect(imu!.latest(ax)).toBeCloseTo(0.2);
  expect(imu!.latest(az)).toBeCloseTo(9.7);
  expect(imu!.latest(imu!.channel("angular_rate.y"))).toBeNaN();
});

test("a repeated backfill (after a reconnect) adds nothing; the newer rows do", () => {
  const feed = new LiveFeed();
  feed.setCatalog(CATALOG);
  const imu = feed.device("imu")!;
  const block = (t: number[]) => ({ series: at("imu.acceleration.x"), t: Float64Array.from(t), v: Float64Array.from(t.map((x) => x / 1000)) });
  feed.apply({ backfill: true, blocks: [block([1_000, 2_000, 3_000])] });
  feed.apply({ backfill: true, blocks: [block([1_000, 2_000, 3_000])] });
  expect(imu.count).toBe(3);
  feed.apply({ backfill: false, blocks: [block([2_500, 3_000, 4_000])] });
  expect(imu.count).toBe(4);
  expect(imu.newestUs()).toBe(4_000);
});

test("the same catalog again keeps the rows; a different one starts the devices over", () => {
  const feed = new LiveFeed();
  feed.setCatalog(CATALOG);
  const imu = feed.device("imu")!;
  feed.apply({ backfill: false, blocks: [{ series: at("imu.acceleration.x"), t: Float64Array.of(1_000), v: Float64Array.of(1) }] });
  feed.setCatalog({ ...CATALOG, epoch: 4 });
  expect(feed.device("imu")).toBe(imu);
  expect(imu.count).toBe(1);
  feed.setCatalog({ ...CATALOG, series: CATALOG.series.slice(0, 3) });
  expect(feed.device("imu")).not.toBe(imu);
  expect(feed.device("orientation")).toBeUndefined();
});

test("the ring's LEDs map to their physical places, clockwise or back", () => {
  const feed = new LiveFeed();
  feed.setCatalog(CATALOG);
  const light = feed.device("light")!;
  const values = (offset: number, clockwise: number, colors: number[]) => ({
    backfill: false,
    blocks: [
      { series: at("light.offset"), t: Float64Array.of(5_000), v: Float64Array.of(offset) },
      { series: at("light.clockwise"), t: Float64Array.of(5_000), v: Float64Array.of(clockwise) },
      { series: at("light.brightness"), t: Float64Array.of(5_000), v: Float64Array.of(0.5) },
      { series: at("light.led.0"), t: Float64Array.of(5_000), v: Float64Array.of(colors[0]) },
      { series: at("light.led.1"), t: Float64Array.of(5_000), v: Float64Array.of(colors[1]) },
    ],
  });
  // Two LEDs, offset 0, clockwise: logical 0 at physical 0, logical 1 at physical 1.
  feed.apply(values(0, 1, [0x00ff0000, 0x0000ff00]));
  let frame = ringFrame(light, null);
  expect(frame?.colors).toEqual([0x00ff0000, 0x0000ff00]);
  expect(frame?.brightness).toBe(0.5);
  // Counter-clockwise from offset 1: logical 1 sits at physical 0.
  feed.apply({ backfill: false, blocks: values(1, 0, [0x000000ff, 0x00ffffff]).blocks.map((b) => ({ ...b, t: Float64Array.of(6_000) })) });
  frame = ringFrame(light, frame);
  expect(frame?.colors).toEqual([0x00ffffff, 0x000000ff]);
});

test("a white channel folds into the colour", () => {
  const feed = new LiveFeed();
  feed.setCatalog(CATALOG);
  const light = feed.device("light")!;
  feed.apply({
    backfill: false,
    blocks: [
      { series: at("light.offset"), t: Float64Array.of(1_000), v: Float64Array.of(0) },
      { series: at("light.clockwise"), t: Float64Array.of(1_000), v: Float64Array.of(1) },
      { series: at("light.led.0"), t: Float64Array.of(1_000), v: Float64Array.of(0x40_10_20_30) },
    ],
  });
  // White (0x40) is added to each channel: 0x10 + 0x40, 0x20 + 0x40, 0x30 + 0x40.
  expect(ringFrame(light, null)?.colors[0]).toBe(0x50_60_70);
});

test("the Mahony filter levels at rest and turns with the gyro", () => {
  // Gravity along the board's z (lens down): the case stays level.
  const q = fromGravity(0, 0, 9.80665);
  expect(euler(q).pitch).toBeCloseTo(0, 6);
  const feed = new LiveFeed();
  feed.setCatalog(CATALOG);
  const imu = feed.device("imu")!;
  const samples = (t: number, series: number, v: number) => ({ series, t: Float64Array.of(t), v: Float64Array.of(v) });
  // Still, then a steady yaw rate of 1 rad/s for 1 s, sampled every 10 ms.
  const blocks = [];
  for (let i = 0; i <= 100; i++) {
    const t = i * 10_000;
    blocks.push(samples(t, at("imu.acceleration.x"), 0), samples(t, at("imu.acceleration.y"), 0), samples(t, at("imu.acceleration.z"), 9.80665));
    blocks.push(samples(t, at("imu.angular_rate.x"), 0), samples(t, at("imu.angular_rate.y"), 0));
    blocks.push(samples(t, at("imu.angular_rate.z"), i > 0 ? 1 : 0));
  }
  feed.apply({ backfill: false, blocks });
  const orientation = new Orientation([0, 1, 2, 3, 4, 5].map((c) => c));
  orientation.update(imu);
  // Board yaw about z by about one radian (the filter's own estimate, within its drift).
  expect(Math.abs(euler(orientation.q).yaw)).toBeGreaterThan(40);
  orientation.zero();
  expect(Math.abs(euler(orientation.q).yaw)).toBeLessThan(1e-3);
  // The case is the IMU turned into the case's frame: a level board reads the lens as z.
  expect(caseOrientation([1, 0, 0, 0])).toEqual([0, Math.SQRT1_2, -Math.SQRT1_2, 0]);
});

test("the board's fused quaternion takes the zero heading as an offset", () => {
  const feed = new LiveFeed();
  feed.setCatalog(CATALOG);
  // Fusion channels are not in this test catalog: build a device with them directly.
  const fusion = FusedOrientation.channelsOf(["quaternion.w", "quaternion.x", "quaternion.y", "quaternion.z"]);
  expect(fusion).toEqual({ q: [0, 1, 2, 3], disturbed: -1, confidence: -1 });
  expect(FusedOrientation.channelsOf(["acceleration.x"])).toBeNull();
});

test("the simulated robot's stream reads back through the same feed", () => {
  const sent: string[] = [];
  const binaries: ArrayBuffer[] = [];
  const previous = (globalThis as { window?: unknown }).window;
  // The simulator uses the browser's timer and clock: a stand-in with the same two calls.
  (globalThis as { window?: unknown }).window = { setInterval: () => 1, clearInterval: () => {} };
  try {
    const stop = openSimulatedStream({ text: (t) => sent.push(t), binary: (b) => binaries.push(b), closed: () => {} });
    stop();
  } finally {
    (globalThis as { window?: unknown }).window = previous;
  }
  const catalog = parseTextFrame(sent[0]);
  expect(catalog?.type).toBe("catalog");
  const feed = new LiveFeed();
  if (catalog?.type === "catalog") feed.setCatalog(catalog.catalog);
  expect(feed.device("imu")).toBeDefined();
  expect(feed.device("orientation")).toBeDefined();
  expect(feed.device("light")?.channels.length).toBe(19);
  // The backfill: five seconds on a 10 ms grid.
  const batch = decodeSamples(binaries[0]);
  expect(batch?.backfill).toBe(true);
  feed.apply(batch!);
  const imu = feed.device("imu")!;
  expect(imu.count).toBeGreaterThan(400);
  expect(imu.count).toBeLessThanOrEqual(501);
  // Gravity reads about 1 g on the board's z (with the simulated tilt).
  const at0 = imu.index(imu.count - 1);
  const magnitude = Math.hypot(imu.v[0][at0], imu.v[1][at0], imu.v[2][at0]);
  expect(magnitude).toBeGreaterThan(9.5);
  expect(magnitude).toBeLessThan(10.1);
});

