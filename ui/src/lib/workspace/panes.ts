// The pane registry: every kind of pane a workspace can hold. A pane is a
// component that gets its own `PaneRef` (id, type, props) and the workspace
// id; it puts its tools in the tab row with <PaneBar>.

import type { Component } from "svelte";
import type { PaneRef } from "#lib/core/workspace.svelte.js";
import type { IconName } from "#lib/ui/icons.js";

export interface PaneProps {
  pane: PaneRef;
  ws: string;
}

export interface PaneDef {
  type: string;
  title: string;
  icon: IconName;
  group: "Vision" | "Pipelines" | "Robot" | "Hardware" | "System" | "Tools";
  summary: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- panes that ignore their props are fine too
  component: Component<PaneProps> | Component<any>;
  /** Tab title from the pane's props (e.g. the camera it is pinned to). */
  label?: (props: Record<string, unknown>) => string | null;
  /** Identity id the tab takes its colour from. */
  identityOf?: (props: Record<string, unknown>) => string | null;
}

const registry = new Map<string, PaneDef>();

export function definePane(def: PaneDef) {
  registry.set(def.type, def);
}

export function paneDef(type: string): PaneDef | undefined {
  return registry.get(type);
}

export function allPanes(): PaneDef[] {
  return [...registry.values()];
}
