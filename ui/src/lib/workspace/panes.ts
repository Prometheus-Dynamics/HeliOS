// The pane registry: every kind of pane a workspace can hold. A pane is a
// component that gets its own `PaneRef` (id, type, props) and the workspace
// id; it puts its tools in the tab row with <PaneBar>.

import type { Component } from "svelte";
import type { PaneRef } from "#lib/core/workspace.svelte.js";
import type { FeatureName } from "#lib/api/types.js";
import type { IconName } from "#lib/ui/icons.js";
import { device } from "#lib/stores/device.svelte.js";

export interface PaneProps {
  pane: PaneRef;
  ws: string;
}

/** The job a pane belongs to: the built-in screen it is found on (docs/docs/ui-information-architecture.md). */
export type Job = "setup" | "run" | "tune" | "device";

export const JOB_NAMES: Record<Job, string> = { setup: "Set up", run: "Run", tune: "Tune", device: "Device" };

export interface PaneDef {
  type: string;
  title: string;
  icon: IconName;
  group: Job;
  summary: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- panes that ignore their props are fine too
  component: Component<PaneProps> | Component<any>;
  /** Tab title from the pane's props (e.g. the camera it is pinned to). */
  label?: (props: Record<string, unknown>) => string | null;
  /** Identity id the tab takes its colour from. */
  identityOf?: (props: Record<string, unknown>) => string | null;
  /** The device feature the pane is for: it is offered (galleries, palette) only where the device has it. */
  requires?: FeatureName;
}

const registry = new Map<string, PaneDef>();

export function definePane(def: PaneDef) {
  registry.set(def.type, def);
}

export function paneDef(type: string): PaneDef | undefined {
  return registry.get(type);
}

/** The built-in screen a pane lives on. */
export function homeOf(def: PaneDef): Job {
  return def.group;
}

/** The panes this device offers: those whose feature it has. */
export function allPanes(): PaneDef[] {
  return [...registry.values()].filter((def) => !def.requires || device.has(def.requires));
}
