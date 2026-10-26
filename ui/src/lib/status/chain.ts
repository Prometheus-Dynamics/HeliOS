// A camera's status chain (`GET /v1/cameras/{id}/status`, in each camera of `GET /v1/cameras`) as the
// UI reads it: the stage it stops at, the stages a pane shows, and the line that says why. Pure, so
// the panes, the attention list and the tests read it the same way.

import type { CameraStatus, StageState, StatusStage } from "#lib/api/types.js";
import type { Tone } from "#lib/format.js";

/** The stage a camera's chain stops at (`null` when every stage that applies is ok). */
export function blockingStage(status: CameraStatus | null | undefined): StatusStage | null {
  if (!status?.blocking) return null;
  return status.stages.find((s) => s.stage === status.blocking) ?? null;
}

/** The stages to show, in chain order: the ones that apply (not `off`), or only the named ones. */
export function visibleStages(status: CameraStatus | null | undefined, only?: readonly string[]): StatusStage[] {
  if (!status) return [];
  return status.stages.filter((s) => s.state !== "off" && (!only || only.includes(s.stage)));
}

/** A stage's line for the screen: what it is doing or why it is not working, and what to do. */
export function whyOf(stage: StatusStage): { title: string; hint: string | null } {
  return { title: `${stage.label}: ${stage.reason}`, hint: stage.hint };
}

/** A stage's progress as a fraction of its target, 0 to 1 (0 without a target). */
export function fraction(progress: StatusStage["progress"]): number {
  if (!progress || progress.target <= 0) return 0;
  return Math.min(1, Math.max(0, progress.value / progress.target));
}

/** The tone a state is drawn in. */
export const STATE_TONE: Record<StageState, Tone> = { ok: "success", waiting: "info", blocked: "error", off: "neutral" };
