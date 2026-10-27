// Problems the robot has right now, each with the fixes that apply.

import type { IconName } from "#lib/ui/icons.js";
import { drafts } from "#lib/graph/drafts.svelte.js";
import { shell } from "#lib/core/shell.svelte.js";
import { cluster } from "#lib/stores/cluster.svelte.js";
import { blockingStage } from "#lib/status/chain.js";

export interface Problem {
  id: string;
  severity: "error" | "warning";
  title: string;
  why: string;
  fixes: { label: string; icon: IconName; run: () => void; primary?: boolean }[];
}

/** Where each stage of a camera's status chain is fixed (the same panes as the setup checklist). */
const STAGE_FIX: Record<string, { label: string; icon: IconName; screen: "setup" | "tune" | "device"; pane: string }> = {
  camera: { label: "Open devices", icon: "camera", screen: "device", pane: "devices" },
  service: { label: "Open devices", icon: "camera", screen: "device", pane: "devices" },
  frames: { label: "Open devices", icon: "camera", screen: "device", pane: "devices" },
  pipeline: { label: "Choose what it runs", icon: "list-check", screen: "setup", pane: "camera-jobs" },
  detections: { label: "Open camera controls", icon: "adjustments-horizontal", screen: "tune", pane: "camera-controls" },
  calibration: { label: "Open calibration", icon: "grid-4x4", screen: "setup", pane: "calibration" },
  map: { label: "Choose what it runs", icon: "list-check", screen: "setup", pane: "camera-jobs" },
  depth: { label: "Choose what it runs", icon: "list-check", screen: "setup", pane: "camera-jobs" },
  features: { label: "Choose what it runs", icon: "list-check", screen: "setup", pane: "camera-jobs" },
};

/** Needs a person: an error, or something with a fix to press. */
export const actionable = (p: Problem) => p.severity === "error" || p.fixes.length > 0;

export function problems(): Problem[] {
  const out: Problem[] = [];
  for (const w of cluster.workloads.filter((x) => x.state === "quarantined")) {
    const stop = cluster.stopOf(w);
    const fix = stop.stage ? STAGE_FIX[stop.stage] : undefined;
    out.push({
      id: `wl:${w.id}`,
      severity: "error",
      title: `${w.name}: ${stop.reason}`,
      why: stop.hint ?? "Stopped to protect the rest of the device.",
      fixes: [
        ...(fix ? [{ label: fix.label, icon: fix.icon, primary: true, run: () => shell.open(fix.screen, fix.pane) }] : []),
        // A bad revision is what rolling back and restarting mend; a missing camera or a blocked stage is not.
        ...(stop.restart && cluster.canRollBack(w.id) ? [{ label: `Roll back to r${w.revision - 1}`, icon: "history" as const, primary: true, run: () => drafts.get(w.id).rollBack() }] : []),
        ...(stop.restart ? [{ label: "Restart", icon: "refresh" as const, run: () => cluster.restartWorkload(w.id) }] : []),
        { label: "Stop", icon: "player-stop", run: () => cluster.stopWorkload(w.id) },
      ],
    });
  }
  for (const c of cluster.cameras) {
    const stop = blockingStage(c.status);
    if (stop?.state === "blocked") out.push({ id: `cam:${c.resourceId}`, severity: "error", title: `${c.name}: ${stop.reason}`, why: stop.hint ?? "", fixes: [] });
  }
  for (const n of cluster.nodes.filter((x) => x.health === "offline")) {
    out.push({ id: `off:${n.id}`, severity: "error", title: `${n.name}: offline`, why: "No heartbeat. Check power and link; it rejoins on its own.", fixes: [{ label: "Reboot", icon: "power", run: () => cluster.reboot(n.id) }] });
  }
  for (const n of cluster.nodes.filter((x) => x.health === "degraded")) {
    const flaky = n.services.filter((s) => s.restarts > 0 || s.state !== "running");
    out.push({
      id: `deg:${n.id}`,
      severity: "warning",
      title: `${n.name}: degraded`,
      why: flaky.map((s) => `${s.name} restarted ${s.restarts}×`).join(", ") || "A service is unhealthy.",
      fixes: flaky.map((s) => ({ label: `Restart ${s.name}`, icon: "refresh" as const, run: () => cluster.restartService(n.id, s.name) })),
    });
  }
  for (const n of cluster.nodes.filter((x) => x.clock.offsetUs > 1000 && x.health !== "offline")) {
    out.push({
      id: `clk:${n.id}`,
      severity: "warning",
      title: `${n.name}: clock offset ${(n.clock.offsetUs / 1000).toFixed(1)} ms`,
      why: `${n.clock.source} sync; timestamps skewed until it converges.`,
      fixes: [],
    });
  }
  return out;
}
