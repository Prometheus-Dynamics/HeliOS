import { describe, expect, test } from "bun:test";
import { formatReading, isLinkChannel, LEMNOS_DEVICE_TYPE, lemnosDevices, statusTone } from "../src/lib/api/lemnos-devices";
import type { Resource } from "../src/lib/api/types";

const resource = (over: Partial<Resource>): Resource => ({
  id: "lemnos.raze.cpu-thermal",
  type: LEMNOS_DEVICE_TYPE,
  name: "cpu-thermal",
  kind: null,
  provider: "lemnos",
  health: "healthy",
  availability: "available",
  ownership: "shared_read",
  lease_state: "unleased",
  leased_by: [],
  workload: null,
  capabilities: [],
  labels: { "lemnos.board": "raze", "lemnos.class": "temperature" },
  endpoints: [],
  action_result: null,
  state: { temperature: 57.85, status: "available", reason: "", read_us: 1200 },
  ...over,
});

describe("lemnosd devices on the Hardware pane", () => {
  test("maps each bridge resource to its device id, class, status and readings", () => {
    const devices = lemnosDevices([
      resource({}),
      resource({
        id: "lemnos.raze.can-s0",
        name: "CAN s0",
        labels: { "lemnos.board": "raze", "lemnos.class": "other", "lemnos.model": "can" },
        state: { carrier: 1, status: "available", reason: "" },
      }),
    ]);
    expect(devices).toEqual([
      { id: "cpu-thermal", name: "cpu-thermal", cls: "temperature", status: "available", reason: "", readings: [{ channel: "temperature", value: 57.85 }] },
      { id: "can-s0", name: "CAN s0", cls: "other", status: "available", reason: "", readings: [{ channel: "carrier", value: 1 }] },
    ]);
  });

  test("ignores resources of other types", () => {
    const devices = lemnosDevices([
      resource({ type: "lemnos.raw", id: "lemnos_raw_node-local_io", labels: {} }),
      resource({ type: "camera.device", id: "capture_device_node-local_video0", labels: {} }),
    ]);
    expect(devices).toEqual([]);
  });

  test("keeps only channel readings, and the status and reason entries", () => {
    const [device] = lemnosDevices([
      resource({
        id: "lemnos.raze.imu",
        labels: { "lemnos.board": "raze", "lemnos.class": "imu" },
        state: {
          "acceleration.x": 0.02,
          "lemnos.note": "text",
          bad: Number.NaN,
          read_us: 12,
          "control.duty": 0.5,
          "fan.override.active": false,
          status: "degraded",
          reason: "read: timed out",
        },
      }),
    ]);
    expect(device.id).toBe("imu");
    expect(device.status).toBe("degraded");
    expect(device.reason).toBe("read: timed out");
    expect(device.readings).toEqual([{ channel: "acceleration.x", value: 0.02 }]);
  });

  test("a device with no state yet has no readings and falls back to the availability", () => {
    const [device] = lemnosDevices([resource({ labels: {}, availability: "unavailable", state: null })]);
    expect(device).toEqual({ id: "raze.cpu-thermal", name: "cpu-thermal", cls: "", status: "unavailable", reason: "", readings: [] });
  });
});

describe("reading and status presentation", () => {
  test("a temperature is shown in degrees Celsius", () => {
    expect(formatReading("temperature", 57.85)).toBe("57.9 °C");
  });

  test("a link flag is shown as up or down", () => {
    expect(isLinkChannel("carrier")).toBe(true);
    expect(isLinkChannel("link-up")).toBe(true);
    expect(isLinkChannel("temperature")).toBe(false);
    expect(formatReading("carrier", 1)).toBe("up");
    expect(formatReading("carrier", 0)).toBe("down");
    expect(formatReading("link_up", 1)).toBe("up");
  });

  test("other readings are shown as numbers", () => {
    expect(formatReading("count", 4)).toBe("4");
    expect(formatReading("acceleration.x", 0.0234)).toBe("0.02");
  });

  test("status tones", () => {
    expect(statusTone("available")).toBe("ok");
    expect(statusTone("degraded")).toBe("warn");
    expect(statusTone("missing")).toBe("err");
    expect(statusTone("disconnected")).toBe("neutral");
  });
});
