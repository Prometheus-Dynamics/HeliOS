// A pipeline's Daedalus plan (`GET /v1/pipelines/{id}/plan`, explain_plan), read once per revision
// and shared by the graph (which marks the transfers on its edges) and the plan list under it. A
// device transfer is an edge with an adapter step (a DeviceUpload or DeviceDownload) or one that
// crosses residency: each one is a round trip the graph pays for (AGENTS.md rule 4).

import { errorText } from "#lib/api/client.js";
import type { PipelinePlan, PlanEdge } from "#lib/api/types.js";
import type { PipelineDevice } from "#lib/pipeline/device.js";

export interface Transfer {
  edge: PlanEdge;
  /** What the edge does to the frame: up to the GPU, down to the CPU, or across residency. */
  kind: "upload" | "download" | "cross";
  /** The adapter steps as text. */
  steps: string;
}

export interface PlanState {
  plan: PipelinePlan | null;
  error: string | null;
  loading: boolean;
  /** The revision the plan was read for. */
  revision: number | null;
}

/** The transfers of a plan, with what each one is. */
export function transfersOf(plan: PipelinePlan | null): Transfer[] {
  if (!plan) return [];
  return plan.plan.edges
    .filter((e) => e.adapter_steps.length > 0 || e.crosses_residency)
    .map((edge) => {
      const steps = edge.adapter_steps.map((s) => (typeof s === "string" ? s : JSON.stringify(s))).join(", ");
      const kind = /upload/i.test(steps) ? "upload" : /download/i.test(steps) ? "download" : "cross";
      return { edge, kind, steps };
    });
}

/** The key an edge is looked up by, the same for a plan edge and a graph edge. */
export const transferKey = (fromNode: string, fromPort: string, toNode: string, toPort: string) => `${fromNode}.${fromPort}>${toNode}.${toPort}`;

class PlanStore {
  #plans = $state<Record<string, PlanState>>({});

  get(id: string): PlanState {
    return this.#plans[id] ?? { plan: null, error: null, loading: false, revision: null };
  }

  /**
   * Reads the plan for a revision (a no-op while that revision's plan is here or on its way). `id` is
   * the pipeline's cluster key; `device` is where it runs (a peer's plan is read through the gateway).
   */
  async load(id: string, revision: number, device: PipelineDevice) {
    const current = this.#plans[id];
    if (current && current.revision === revision && (current.loading || current.plan || current.error)) return;
    this.#plans[id] = { plan: current?.plan ?? null, error: null, loading: true, revision };
    try {
      const plan = await device.scope.pipelinePlan(device.id);
      this.#plans[id] = { plan, error: null, loading: false, revision };
    } catch (error) {
      this.#plans[id] = { plan: null, error: errorText(error), loading: false, revision };
    }
  }
}

export const plans = new PlanStore();
