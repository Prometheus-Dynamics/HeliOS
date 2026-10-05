// Problems the robot has right now, each with the fixes that apply.

import type { IconName } from "$lib/ui/icons";
import { drafts } from "$lib/graph/drafts.svelte";
import { cluster } from "$lib/stores/cluster.svelte";

export interface Problem {
  id: string;
  severity: "error" | "warning";
  title: string;
  why: string;
  fixes: { label: string; icon: IconName; run: () => void; primary?: boolean }[];
}

/** Needs a person: an error, or something with a fix to press. */
export const actionable = (p: Problem) => p.severity === "error" || p.fixes.length > 0;

export function problems(): Problem[] {
  const out: Problem[] = [];
  for (const w of cluster.workloads.filter((x) => x.state === "quarantined")) {
    out.push({
      id: `wl:${w.id}`,
      severity: "error",
      title: `${w.name}: quarantined after ${w.restarts} crashes`,
      why: "Stopped to protect the rest of the device; its camera keeps streaming.",
      fixes: [
        ...(cluster.canRollBack(w.id) ? [{ label: `Roll back to r${w.revision - 1}`, icon: "history" as const, primary: true, run: () => drafts.get(w.id).rollBack() }] : []),
        { label: "Restart", icon: "refresh", run: () => cluster.restartWorkload(w.id) },
        { label: "Stop", icon: "player-stop", run: () => cluster.stopWorkload(w.id) },
      ],
    });
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
