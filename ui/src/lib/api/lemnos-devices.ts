// lemnosd's devices as the Hardware pane shows them: one resource per device, published by Lemnos's
// Orion bridge (`lemnos.device`, `lemnos.<board>.<device>`; Lemnos docs/orion.md). Its readings
// are its state: one number per channel (a thermal zone's `temperature` in °C, a CAN link's
// `carrier`), next to its `status`, `reason`, `read_us` and `control.<name>` values. A fan takes
// helios-api's timed `fan.override` / `fan.release` on its own resource.

import type { Resource } from "./types";

/** The resource type of Lemnos's Orion bridge: one per lemnosd device. */
export const LEMNOS_DEVICE_TYPE = "lemnos.device";

/** State entries of a bridge resource that are not channel readings. */
function isReading(key: string): boolean {
  return key !== "read_us" && !key.startsWith("control.") && !key.startsWith("fan.override.");
}

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
        .filter((entry): entry is [string, number] => isReading(entry[0]) && typeof entry[1] === "number" && Number.isFinite(entry[1]))
        .map(([channel, value]) => ({ channel, value }));
      const board = r.labels["lemnos.board"];
      const prefix = board ? `lemnos.${board}.` : "lemnos.";
      return {
        id: r.id.startsWith(prefix) ? r.id.slice(prefix.length) : r.id,
        name: r.name,
        cls: r.labels["lemnos.class"] ?? "",
        status: typeof state.status === "string" ? state.status : r.availability,
        reason: typeof state.reason === "string" ? state.reason : "",
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
