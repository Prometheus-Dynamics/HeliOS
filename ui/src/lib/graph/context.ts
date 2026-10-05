// Live facts the canvas nodes read without being pushed into node data (so a
// timing tick re-renders node faces, not the whole flow).
import { getContext, setContext } from "svelte";

export interface EditorLive {
  /** Per-node milliseconds, by graph node id. */
  readonly nodeMs: Record<string, number>;
  /** Denominator for a node's share of the tick. */
  readonly tickMs: number;
  readonly running: boolean;
}

const KEY = Symbol("graph-editor-live");

export function setEditorLive(live: EditorLive) {
  setContext(KEY, live);
}

export function getEditorLive(): EditorLive | undefined {
  return getContext<EditorLive | undefined>(KEY);
}
