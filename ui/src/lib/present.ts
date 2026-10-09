// How model values present: icons, tones and labels shared by screens.
import type { DotState } from "#lib/components/common/StatusDot.svelte";
import type { Tone } from "#lib/format.js";
import type { Health, NodeKind, ResourceType, WorkloadState } from "#lib/api/model.js";
import type { IconName } from "#lib/ui/icons.js";

export function nodeIcon(kind: NodeKind): IconName {
  return { raze: "camera", mcu: "circuit-cell", coprocessor: "device-laptop", foreign: "box-multiple" }[kind] as IconName;
}

export function healthDot(health: Health): DotState {
  return { online: "online", degraded: "busy", offline: "offline" }[health] as DotState;
}

export function healthTone(health: Health): Tone {
  return { online: "success", degraded: "warning", offline: "error" }[health] as Tone;
}

export function workloadTone(state: WorkloadState): Tone {
  return { running: "success", starting: "info", quarantined: "error", stopped: "neutral" }[state] as Tone;
}

export function resourceIcon(type: ResourceType): IconName {
  const map: Record<ResourceType, IconName> = {
    camera: "camera",
    stream: "broadcast",
    gpio: "plug-connected",
    pwm: "wave-sine",
    imu: "crosshair",
    fan: "propeller",
    led: "bulb",
    power: "bolt",
    compute: "cpu-2",
    bus: "route",
    usb: "usb",
    other: "plug-connected",
  };
  return map[type];
}
