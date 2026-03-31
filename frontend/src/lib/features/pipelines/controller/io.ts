import { PipelinesApi } from '$lib/api/pipelinesApi';
import { StreamsApi } from '$lib/api/streamsApi';

export async function fetchPipelineGraph(pipelineId: string) {
  return PipelinesApi.fetchGraph({ id: pipelineId });
}

export async function fetchPipelineTemplate(templateId: string) {
  return PipelinesApi.fetchTemplate({ id: templateId });
}

export async function validatePipelineGraph(requestBody: {
  graph: unknown;
  enable_lints?: boolean;
  active_features?: string[];
  graph_id?: string | null;
}) {
  return PipelinesApi.validateGraph({ requestBody });
}

export async function uploadPipelineGraph(graph: unknown, name?: string | null) {
  return PipelinesApi.uploadGraph({ requestBody: { graph, name: name ?? null } });
}

export async function listCaptureBackends() {
  return StreamsApi.resolvedStreams();
}

export async function listPipelineRegistry() {
  return PipelinesApi.listRegistry();
}
