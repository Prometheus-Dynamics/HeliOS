import type { Edge, Node } from '@xyflow/svelte';
import type { PipelineGraphPlan } from '$lib/types/pipeline';
import type { HistoryManager } from './historyManager';

export function commitGraphPlan(options: {
  interactive: boolean;
  history: HistoryManager;
  plan: PipelineGraphPlan;
  nodes: Node[];
  edges: Edge[];
  edgeInteractionsEnabled: boolean;
  rebuildPlan: (plan: PipelineGraphPlan, nodes: Node[], edges: Edge[], edgeInteractionsEnabled: boolean) => PipelineGraphPlan;
  ensurePlanPortMetadata: (plan: PipelineGraphPlan) => PipelineGraphPlan;
  setPlan: (plan: PipelineGraphPlan) => void;
  dispatchChange: (plan: PipelineGraphPlan) => void;
}): void {
  if (!options.interactive) return;
  if (options.history.isEmpty()) {
    options.history.setSnapshot(options.plan);
  }
  const rebuilt = options.ensurePlanPortMetadata(
    options.rebuildPlan(options.plan, options.nodes, options.edges, options.edgeInteractionsEnabled)
  );
  options.setPlan(rebuilt);
  options.history.pushSnapshot(rebuilt);
  options.dispatchChange(rebuilt);
}
