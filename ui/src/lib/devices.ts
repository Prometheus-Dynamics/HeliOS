// Device vocabulary shared by the device screens: recovery levels, kinds, links.
import type { Link, NodeKind, RecoveryLevel } from "$lib/api/model";
import type { IconName } from "$lib/ui/icons";

export const RECOVERY: Record<RecoveryLevel, { title: string; detail: string }> = {
  R1: { title: "Faults contained", detail: "A crashing service or pipeline is restarted or quarantined; the rest of the device keeps running." },
  R2: { title: "Bad update reverts itself", detail: "Updates boot into the spare slot on trial; if the health check fails, the device falls back to the last good slot." },
  R3: { title: "Reachable when both images are bad", detail: "A minimal recovery image keeps the agent reachable so a fresh image can be written over the network." },
  R4: { title: "Hardware path", detail: "A physical path (USB boot, SWD, BOOTSEL) can always reflash the device, even with no working software." },
};

export const KIND_LABEL: Record<NodeKind, { title: string; subtitle: string }> = {
  raze: { title: "Razes", subtitle: "Camera coprocessors running HeliOS" },
  mcu: { title: "Microcontrollers", subtitle: "Firmware nodes on CAN and UART, managed by the agent" },
  coprocessor: { title: "Coprocessors", subtitle: "General computers that joined the cluster" },
  foreign: { title: "Foreign devices", subtitle: "Other vision systems, connected through an adapter" },
};

export const LINK_ICON: Record<Link, IconName> = { ethernet: "network", usb: "usb", can: "route", uart: "plug-connected" };
