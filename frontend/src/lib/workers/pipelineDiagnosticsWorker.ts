import { buildGraphDiagnostics } from '$lib/components/flow/pipeline-graph/diagnostics';
import type { PipelineDiagnostics, PipelineGraphPlan } from '$lib/types/pipeline';
import type { PipelineGraphDiagnostics } from '$lib/components/flow/pipeline-graph/types';

type Payload = {
  requestId: number;
  graph?: PipelineGraphPlan | null;
  diagnostics?: PipelineDiagnostics | null;
};

self.onmessage = (event: MessageEvent<Payload>) => {
  const { requestId, graph, diagnostics } = event.data ?? { requestId: 0 };
  const map: PipelineGraphDiagnostics =
    graph && diagnostics ? buildGraphDiagnostics(graph, diagnostics) : {};
  self.postMessage({ requestId, map });
};
