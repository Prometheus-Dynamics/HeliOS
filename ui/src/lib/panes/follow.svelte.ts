// Panes either follow the app selection ("the camera you clicked last") or
// are pinned to one object through their props.

import { selection, type SelectionKind } from "#lib/core/selection.svelte.js";
import { workspaces, type PaneRef } from "#lib/core/workspace.svelte.js";

/** `pane` and `ws` are getters so the helper always reads the current props. */
export function follow(paneOf: () => PaneRef, wsOf: () => string, kind: SelectionKind, fallback: () => string | undefined) {
  return {
    get id(): string | undefined {
      return (paneOf().props?.[kind] as string | undefined) ?? selection.last[kind]?.id ?? fallback();
    },
    get pinned(): boolean {
      return Boolean(paneOf().props?.[kind]);
    },
    pin(id: string | undefined) {
      workspaces.setProps(wsOf(), paneOf().id, { [kind]: id });
    },
    toggle() {
      this.pin(this.pinned ? undefined : this.id);
    },
  };
}

/**
 * A pane that shows one pipeline (its graph, its settings, its profile). Pinned, it shows its
 * pipeline. Otherwise it follows the most recent selection that belongs to a pipeline: a pipeline,
 * a node of one, or the camera (its pipeline is `pipelineOf` the camera). A camera that runs no
 * pipeline gives no id, so the pane says so instead of showing an older pipeline.
 */
export function followPipeline(paneOf: () => PaneRef, wsOf: () => string, pipelineOf: (camera: string) => string | undefined, fallback: () => string | undefined) {
  return {
    get id(): string | undefined {
      const pinned = paneOf().props?.workload as string | undefined;
      if (pinned) return pinned;
      const { workload, "graph-node": node, camera } = selection.last;
      const latest = [
        workload && { at: workload.at ?? 0, id: workload.id },
        node && { at: node.at ?? 0, id: node.parent },
        camera && { at: camera.at ?? 0, id: pipelineOf(camera.id) },
      ]
        .filter((choice) => choice !== undefined)
        .sort((a, b) => b.at - a.at)[0];
      return latest ? latest.id : fallback();
    },
    get pinned(): boolean {
      return Boolean(paneOf().props?.workload);
    },
    pin(id: string | undefined) {
      workspaces.setProps(wsOf(), paneOf().id, { workload: id });
    },
    toggle() {
      this.pin(this.pinned ? undefined : this.id);
    },
  };
}
