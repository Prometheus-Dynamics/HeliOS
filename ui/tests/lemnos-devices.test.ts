import { describe, expect, test } from "bun:test";
import { formatReading, isLinkChannel, LEMNOS_DEVICE_TYPE, lemnosDevices, statusTone } from "../src/lib/api/lemnos-devices";
import type { Resource } from "../src/lib/api/types";

const resource = (over: Partial<Resource>): Resource => ({
  id: "lemnos_device_node-local_cpu-thermal",
  type: LEMNOS_DEVICE_TYPE,
  name: "cpu-thermal",
  kind: null,
  provider: "provider.peripherals.node-local",
  health: "healthy",
  availability: "available",
  ownership: "shared_read",
  lease_state: "unleased",
  leased_by: [],
  workload: null,
  capabilities: [],
  labels: { "lemnos.device_id": "cpu-thermal", "lemnos.class": "temperature", "lemnos.status": "available" },
  endpoints: [],
  action_result: null,
  state: { temperature: 57.85 },
  ...over,
});

describe("lemnosd devices on the Hardware pane", () => {
  test("maps each lemnosd device resource to its id, class, status and readings", () => {
    const devices = lemnosDevices([
      resource({}),
      resource({
        id: "lemnos_device_node-local_can-s0",
        name: "CAN s0",
        labels: { "lemnos.device_id": "can-s0", "lemnos.class": "other", "lemnos.status": "available", "lemnos.model": "can" },
        state: { carrier: 1 },
      }),
    ]);
    expect(devices).toEqual([
      { id: "cpu-thermal", name: "cpu-thermal", cls: "temperature", status: "available", reason: "", readings: [{ channel: "temperature", value: 57.85 }] },
      { id: "can-s0", name: "CAN s0", cls: "other", status: "available", reason: "", readings: [{ channel: "carrier", value: 1 }] },
    ]);
  });

  test("ignores resources of other types, including lemnos.raw and the Orion bridge's lemnos.device", () => {
    const devices = lemnosDevices([
      resource({ type: "helios.fan", id: "helios_fan_node-local_fan", labels: {} }),
      resource({ type: "lemnos.raw", id: "lemnos_raw_node-local_io", labels: {} }),
      resource({ type: "lemnos.device", id: "lemnos.raze.usb-a-power", labels: {} }),
    ]);
    expect(devices).toEqual([]);
  });

  test("keeps only numeric readings, and the status and reason labels", () => {
    const [device] = lemnosDevices([
      resource({
        labels: { "lemnos.device_id": "imu", "lemnos.class": "imu", "lemnos.status": "degraded", "lemnos.reason": "read: timed out" },
        state: { "acceleration.x": 0.02, "lemnos.note": "text", bad: Number.NaN, "read_us": 12 },
      }),
    ]);
    expect(device.status).toBe("degraded");
    expect(device.reason).toBe("read: timed out");
    expect(device.readings).toEqual([
      { channel: "acceleration.x", value: 0.02 },
      { channel: "read_us", value: 12 },
    ]);
  });

  test("a device with no state yet has no readings and falls back to the availability", () => {
    const [device] = lemnosDevices([resource({ labels: {}, availability: "unavailable", state: null })]);
    expect(device).toEqual({ id: "lemnos_device_node-local_cpu-thermal", name: "cpu-thermal", cls: "", status: "unavailable", reason: "", readings: [] });
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
