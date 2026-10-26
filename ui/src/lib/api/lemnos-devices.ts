// lemnosd's devices as the Hardware pane shows them: one read-only resource per device, published
// by helios-peripherals (`helios.lemnos_device`). The device's readings are its state: one number
// per channel (a thermal zone's `temperature` in °C, a CAN link's `carrier`). A device here has no
// writes; a fan is written through its own resource (`helios.fan`).

import type { Resource } from "./types";

/** The resource type helios-peripherals gives each lemnosd device. */
export const LEMNOS_DEVICE_TYPE = "helios.lemnos_device";

export interface LemnosDevice {
  /** lemnosd's device id (`cpu-thermal`, `can-s0`). */
  id: string;
  name: string;
  /** The device class as lemnosd names it (`temperature`, `other`). */
  cls: string;
  /** `available`, `degraded`, `faulted`, `missing`, or `disconnected` while lemnosd is away. */
  status: string;
  reason: string;
  readings: { channel: string; value: number }[];
}

/** The lemnosd devices among the node's resources, in the order they come. */
export function lemnosDevices(resources: Resource[]): LemnosDevice[] {
  return resources
    .filter((r) => r.type === LEMNOS_DEVICE_TYPE)
    .map((r) => {
      const state = r.state ?? {};
      const readings = Object.entries(state)
        .filter((entry): entry is [string, number] => typeof entry[1] === "number" && Number.isFinite(entry[1]))
        .map(([channel, value]) => ({ channel, value }));
      return {
        id: r.labels["lemnos.device_id"] ?? r.id,
        name: r.name,
        cls: r.labels["lemnos.class"] ?? "",
        status: r.labels["lemnos.status"] ?? r.availability,
        reason: r.labels["lemnos.reason"] ?? "",
        readings,
      };
    });
}

/** Whether a channel is a link flag (carrier, link up): shown as up or down. */
export function isLinkChannel(channel: string): boolean {
  return /(^|[.\-_])(carrier|link[-_.]?up|up)$/i.test(channel);
}

/** A reading as the pane shows it: °C for a temperature, up or down for a link flag. */
export function formatReading(channel: string, value: number): string {
  if (isLinkChannel(channel)) return value >= 0.5 ? "up" : "down";
  if (/temp/i.test(channel)) return `${value.toFixed(1)} °C`;
  return Number.isInteger(value) ? String(value) : value.toFixed(2);
}

/** The tone of a device's status badge. */
export function statusTone(status: string): "ok" | "warn" | "err" | "neutral" {
  if (status === "available") return "ok";
  if (status === "degraded") return "warn";
  if (status === "faulted" || status === "missing") return "err";
  return "neutral";
}
