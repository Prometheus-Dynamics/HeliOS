// Node parameters as people read them: a label and a unit from the parameter's name
// (`min_side_px` is "Min side", px), and what a search or a "changed only" filter matches.
// Node declarations carry no labels yet, so the name is the source.

import type { Param } from "#lib/api/catalog.js";
import type { ParamValue } from "#lib/graph/graph.js";

/** Name suffixes that are units: the suffix, the unit shown, and a scale note. */
const UNITS: [suffix: string, unit: string][] = [
  ["_px", "px"],
  ["_ms", "ms"],
  ["_us", "µs"],
  ["_hz", "Hz"],
  ["_deg", "°"],
  ["_rad", "rad"],
  ["_m", "m"],
  ["_mm", "mm"],
  ["_percent", "%"],
  ["_permille", "‰"],
  ["_x1000", "× 1/1000"],
  ["_x100", "× 1/100"],
];

/** Words written the way people write them. */
const WORDS: Record<string, string> = { px: "px", lk: "LK", roi: "ROI", dp: "DP", fps: "FPS", id: "ID", ids: "IDs", nt: "NT", tsdf: "TSDF", ae: "AE", awb: "AWB", isp: "ISP" };

export interface ParamLabel {
  label: string;
  unit?: string;
}

/** `min_side_px` → { label: "Min side", unit: "px" }. */
export function paramLabel(name: string): ParamLabel {
  let base = name;
  let unit: string | undefined;
  for (const [suffix, u] of UNITS) {
    if (base.endsWith(suffix) && base.length > suffix.length) {
      base = base.slice(0, -suffix.length);
      unit = u;
      break;
    }
  }
  const words = base.split("_").filter(Boolean).map((w) => WORDS[w] ?? w);
  const label = words.map((w, i) => (i === 0 && !WORDS[w.toLowerCase()] ? w.charAt(0).toUpperCase() + w.slice(1) : w)).join(" ");
  return { label, unit };
}

/** The parameter is away from its default. */
export function isChangedParam(param: Param, value: ParamValue | undefined): boolean {
  if (value === undefined) return false;
  if (typeof param.default === "number" && typeof value === "number") return Math.abs(param.default - value) > 1e-9;
  return param.default !== value;
}

/** The parameter matches a search (its label, its name or its help). */
export function matchesParam(param: Param, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return paramLabel(param.name).label.toLowerCase().includes(q) || param.name.toLowerCase().includes(q) || param.help.toLowerCase().includes(q);
}
