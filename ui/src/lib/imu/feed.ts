// The node's live stream as devices: each lemnosd device's channels in one ring of rows (one
// time, a value per channel), so an IMU's six channels line up for the attitude filter the way
// they were read. The catalog names a device's series `<device>.<channel>`; a device is found by
// a channel only it has (`acceleration.x` an IMU, `quaternion.w` the fusion, `led.0` a light).

import type { Catalog, SamplesBatch } from "./frame.ts";

/** Rows kept per device: 40 s at 100 Hz. */
export const CAPACITY = 4096;

export type DeviceKind = "imu" | "orientation" | "light" | "magnetometer";

/** The channel that marks each kind of device in the catalog. */
const MARKERS: [DeviceKind, string][] = [
  ["imu", "acceleration.x"],
  ["orientation", "quaternion.w"],
  ["light", "led.0"],
  ["magnetometer", "magnetic_field.x"],
];

export interface DeviceChannel {
  /** The channel's name without its device: `acceleration.x`, `led.3`. */
  name: string;
  unit: string;
}

/** A device's rows: `t[i]` is the time of row i, `v[c][i]` its channel c (NaN when not read). */
export class LiveDevice {
  readonly t = new Float64Array(CAPACITY);
  readonly v: Float64Array[];
  /** Where the next row goes, and how many rows there are. */
  head = 0;
  count = 0;
  /** This computer's clock (ms) when the newest row arrived. */
  arrivedMs = 0;

  constructor(
    readonly kind: DeviceKind,
    /** The device's name in the catalog, before its channel: `lemnos.raze.imu`. */
    readonly prefix: string,
    readonly channels: DeviceChannel[],
  ) {
    this.v = channels.map(() => new Float64Array(CAPACITY).fill(Number.NaN));
  }

  /** The buffer index of the i-th row, oldest first. */
  index(i: number): number {
    return (this.head - this.count + i + CAPACITY) % CAPACITY;
  }

  newestUs(): number {
    return this.count ? this.t[this.index(this.count - 1)] : 0;
  }

  /** The first row at or after `us` (binary search; times only grow). */
  firstFrom(us: number): number {
    let low = 0;
    let high = this.count;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (this.t[this.index(mid)] < us) low = mid + 1;
      else high = mid;
    }
    return low;
  }

  channel(name: string): number {
    return this.channels.findIndex((channel) => channel.name === name);
  }

  /** The newest row's value of a channel, or NaN. */
  latest(channel: number): number {
    return this.count && channel >= 0 ? this.v[channel][this.index(this.count - 1)] : Number.NaN;
  }

  /** Appends a row; `values` holds a value per channel (NaN where not read). */
  push(us: number, values: ArrayLike<number>) {
    this.t[this.head] = us;
    for (let c = 0; c < this.v.length; c++) this.v[c][this.head] = values[c] ?? Number.NaN;
    this.head = (this.head + 1) % CAPACITY;
    this.count = Math.min(this.count + 1, CAPACITY);
    this.arrivedMs = Date.now();
  }
}

interface Route {
  device: LiveDevice;
  channel: number;
}

export class LiveFeed {
  /** One device per kind (the first the catalog has of it). */
  readonly devices = new Map<DeviceKind, LiveDevice>();
  /** Whether the publisher of the stream is there (the catalog's `live`). */
  live = false;
  private routes: (Route | null)[] = [];
  private layout = "";

  /** A catalog: the devices it names. The same layout again keeps the rows (a reconnect). */
  setCatalog(catalog: Catalog) {
    this.live = catalog.live;
    const layout = catalog.series.map((series) => `${series.name}|${series.kind}`).join("\n");
    if (layout === this.layout) return;
    this.layout = layout;
    this.devices.clear();
    this.routes = catalog.series.map(() => null);
    for (const [kind, marker] of MARKERS) {
      if (this.devices.has(kind)) continue;
      const hit = catalog.series.find((series) => series.name.endsWith(`.${marker}`));
      if (!hit) continue;
      const prefix = hit.name.slice(0, hit.name.length - marker.length - 1);
      const own = catalog.series.flatMap((series, index) => (series.name.startsWith(`${prefix}.`) ? [{ series, index }] : []));
      const device = new LiveDevice(
        kind,
        prefix,
        own.map(({ series }) => ({ name: series.name.slice(prefix.length + 1), unit: series.unit ?? "" })),
      );
      own.forEach(({ index }, channel) => (this.routes[index] = { device, channel }));
      this.devices.set(kind, device);
    }
  }

  /** A batch's samples into the devices' rows. A row no newer than the device's newest is a repeat
   * (a backfill replayed after a reconnect) and is skipped. */
  apply(batch: SamplesBatch) {
    const pending = new Map<LiveDevice, Map<number, Float64Array>>();
    for (const block of batch.blocks) {
      const route = this.routes[block.series];
      if (!route) continue;
      let rows = pending.get(route.device);
      if (!rows) pending.set(route.device, (rows = new Map()));
      for (let i = 0; i < block.t.length; i++) {
        const t = block.t[i];
        let row = rows.get(t);
        if (!row) rows.set(t, (row = new Float64Array(route.device.channels.length).fill(Number.NaN)));
        row[route.channel] = block.v[i];
      }
    }
    for (const [device, rows] of pending) {
      const newest = device.newestUs();
      for (const t of [...rows.keys()].sort((a, b) => a - b)) {
        if (t > newest) device.push(t, rows.get(t)!);
      }
    }
  }

  device(kind: DeviceKind): LiveDevice | undefined {
    return this.devices.get(kind);
  }
}
