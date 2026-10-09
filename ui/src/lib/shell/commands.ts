// What the palette can do: go to screens, open panes, jump to any camera,
// pipeline or device, and run actions.

import { exportAll } from "#lib/core/backup.js";
import { commands } from "#lib/core/commands.svelte.js";
import { identity } from "#lib/core/identity.svelte.js";
import { prefs, THEMES } from "#lib/core/prefs.svelte.js";
import { selection } from "#lib/core/selection.svelte.js";
import { shell } from "#lib/core/shell.svelte.js";
import { pane, workspaces } from "#lib/core/workspace.svelte.js";
import { downloadJson } from "#lib/files.js";
import { cluster } from "#lib/stores/cluster.svelte.js";
import { allPanes } from "#lib/workspace/panes.js";
import { gallery } from "./gallery.svelte";
import { help } from "./help.svelte";

export function registerCommands() {
  commands.register("screens", () =>
    workspaces.all.map((w) => {
      const i = workspaces.shown.indexOf(w);
      return { id: `screen:${w.id}`, title: workspaces.isVisible(w.id) ? w.name : `${w.name} (add to rail)`, group: "Screens", icon: w.icon, keywords: w.summary, shortcut: i >= 0 && i < 9 ? String(i + 1) : undefined, run: () => { workspaces.show(w.id); shell.go(w.id); } };
    }),
  );
  commands.register("panes", () =>
    allPanes()
      .filter((p) => p.type !== "welcome")
      .map((p) => ({ id: `pane:${p.type}`, title: `Open ${p.title}`, group: "Panes", icon: p.icon, keywords: `${p.summary} ${p.group}`, run: () => workspaces.open(shell.active, pane(p.type)) })),
  );
  commands.register("objects", () => [
    ...cluster.cameras.map((c) => ({ id: `cam:${c.resourceId}`, title: identity.name(c.resourceId, c.name), group: "Cameras", icon: "camera" as const, keywords: `${c.resourceId} ${c.sensor} camera`, run: () => { selection.select({ kind: "camera", id: c.resourceId }); shell.go("cameras"); } })),
    ...cluster.workloads.map((w) => ({ id: `wl:${w.id}`, title: identity.name(w.id, w.name), group: "Pipelines", icon: "schema" as const, keywords: `${w.id} pipeline graph`, run: () => { selection.select({ kind: "workload", id: w.id }); shell.go("pipelines"); } })),
    ...cluster.nodes.map((n) => ({ id: `dev:${n.id}`, title: identity.name(n.id, n.name), group: "Devices", icon: "cpu" as const, keywords: `${n.model} ${n.address} device`, run: () => { selection.select({ kind: "device", id: n.id }); shell.go("hardware"); } })),
  ]);
  commands.register("actions", () => [
    { id: "act:pause", title: cluster.playing ? "Pause live feeds" : "Resume live feeds", group: "Actions", icon: cluster.playing ? "player-pause" : "player-play", shortcut: "Space", run: () => { cluster.playing = !cluster.playing; } },
    { id: "act:reset-layout", title: "Reset this screen's layout", group: "Actions", icon: "rotate-clockwise", run: () => workspaces.reset(shell.active) },
    { id: "act:reopen", title: "Reopen the last closed pane", group: "Actions", icon: "history", keywords: "undo close restore tab", run: () => workspaces.reopen(shell.active) },
    { id: "act:add-screen", title: "Add a screen…", group: "Actions", icon: "layout-grid", keywords: "rail gallery more screens", run: () => gallery.show("screens") },
    { id: "act:add-kit", title: "Add a kit of panes to this screen…", group: "Actions", icon: "puzzle", keywords: "gallery prebuilt add panes", run: () => gallery.show("kits") },
    { id: "act:explain", title: "What's on this screen?", group: "Help", icon: "info-circle", shortcut: "F1", keywords: "explain help panes", run: () => { help.explain = true; } },
    { id: "act:tour", title: "Take the tour", group: "Help", icon: "rocket", keywords: "tutorial help intro", run: () => help.startTour() },
    { id: "act:onboarding", title: "First-time setup", group: "Help", icon: "list-check", keywords: "onboarding wizard welcome team number", run: () => { help.onboarding = true; } },
    { id: "act:density", title: prefs.density === "compact" ? "Comfortable density" : "Compact density", group: "Actions", icon: "baseline-density-medium", run: () => { prefs.density = prefs.density === "compact" ? "comfortable" : "compact"; } },
    { id: "act:backup", title: "Export everything (backup)", group: "Actions", icon: "download", run: () => downloadJson("helios-config.json", exportAll()) },
    ...cluster.workloads.filter((w) => w.state === "quarantined").map((w) => ({ id: `act:restart:${w.id}`, title: `Restart ${w.name}`, group: "Actions", icon: "refresh" as const, run: () => cluster.restartWorkload(w.id) })),
    ...THEMES.map((t) => ({ id: `theme:${t.id}`, title: `Theme: ${t.name}`, group: "Look", icon: "contrast-2" as const, keywords: t.note, run: () => { prefs.theme = t.id; } })),
  ]);
}
