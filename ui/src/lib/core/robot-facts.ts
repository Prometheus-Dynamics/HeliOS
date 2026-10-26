// The robot facts the UI shows before the device answers, and the one roboRIO address helper on
// the UI side (the device's own is `helios_robot::roborio_address`).

import type { Nt4Settings } from "#lib/api/types.js";

/** NetworkTables settings with nothing set (the device's own defaults). */
export const DEFAULT_NT4: Nt4Settings = { enabled: true, team: null, server: null, port: 5810, prefix: "/helios" };

/** `10.TE.AM.2`, the roboRIO address WPILib uses for a team; `null` outside 1..25599. */
export function roborioAddress(team: number): string | null {
  return Number.isInteger(team) && team >= 1 && team <= 25_599 ? `10.${Math.floor(team / 100)}.${team % 100}.2` : null;
}
