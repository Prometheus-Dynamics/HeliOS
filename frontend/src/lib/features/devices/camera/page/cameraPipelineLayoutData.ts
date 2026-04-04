import type {
  DaedalusRegistryResponse,
  PipelineDocument,
  PipelineTemplateDocument
} from '$lib/api/client';
import type { PipelineGraphPlan } from '$lib/types/pipeline';
import type { PipelineDataType } from '$lib/types/pipeline';
import { collectPipelineOutputs } from '$lib/features/pipelines/boundaryOutputs';
import { fromApiGraphPlan } from '$lib/features/pipelines/graphConverters';
import { extractGraphOutputPortTypes, filterEncoderCompatibleOutputs } from '$lib/features/pipelines/outputFilters';
import { normalizeAssignedPipelineIds } from './cameraPipelineShared';
import { RAW_LOOPBACK_GRAPH, RAW_PIPELINE_ID } from './cameraPipelineShared';
import { extractGraphOutputPorts as extractGraphOutputPortsFn } from './layoutValidation';
import type {
  PipelineGraphAndOutputs,
  PipelineGraphSummary,
  PipelineLayoutDeps,
  PipelineLayoutState,
  PipelineTemplateCreateResult,
  PipelineTemplateEntry
} from './cameraPipelineLayoutTypes';

const stringArraysEqual = (left: string[] | undefined, right: string[]): boolean => {
  if (!Array.isArray(left)) return false;
  if (left.length !== right.length) return false;
  for (let i = 0; i < left.length; i += 1) {
    if (left[i] !== right[i]) return false;
  }
  return true;
};

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

const asTrimmedString = (value: unknown): string => (typeof value === 'string' ? value.trim() : '');

function normalizeGraphDocument(value: unknown): unknown {
  const record = asRecord(value);
  if (!record) return value;
  const nested = asRecord(record.graph) ?? asRecord(record.pipeline_graph) ?? asRecord(record.pipelineGraph);
  if (nested) {
    const baseMetadata = asRecord(record.metadata);
    const nestedMetadata = asRecord(nested.metadata);
    if (baseMetadata && !nestedMetadata) {
      return { ...nested, metadata: baseMetadata };
    }
    if (baseMetadata && nestedMetadata) {
      return { ...nested, metadata: { ...baseMetadata, ...nestedMetadata } };
    }
    return nested;
  }
  return value;
}

function coercePlanFromGraph(graph: unknown): PipelineGraphPlan | null {
  const record = asRecord(graph);
  if (!record) return null;
  const nodes = record.nodes;
  const connections = record.connections;
  if (nodes && typeof nodes === "object" && !Array.isArray(nodes) && Array.isArray(connections)) {
    return graph as PipelineGraphPlan;
  }
  try {
    return fromApiGraphPlan(graph as Parameters<typeof fromApiGraphPlan>[0]);
  } catch {
    return null;
  }
}

function derivePlanOutputs(
  plan: PipelineGraphPlan | null,
  fallbackOutputs: string[]
): { outputs: string[]; types: Record<string, PipelineDataType> } {
  if (!plan) return { outputs: fallbackOutputs, types: {} };
  const boundary = collectPipelineOutputs(plan);
  if (boundary.length > 0) {
    return {
      outputs: boundary.map((entry) => entry.name),
      types: Object.fromEntries(boundary.map((entry) => [entry.name, entry.dataType ?? 'Generic']))
    };
  }

  const hostOutputs = new Set<string>();
  const hostTypes: Record<string, PipelineDataType> = {};
  for (const node of Object.values(plan.nodes ?? {})) {
    const backendId = String(node?.backendId ?? '').toLowerCase();
    if (
      backendId !== 'io.host_output' &&
      !backendId.endsWith(':io.host_output') &&
      backendId !== 'pipeline:output' &&
      !backendId.endsWith(':pipeline:output')
    ) {
      continue;
    }
    const inputs = node?.inputs ?? {};
    Object.entries(inputs).forEach(([port, dataType]) => {
      const trimmed = port.trim();
      if (!trimmed) return;
      hostOutputs.add(trimmed);
      if (!hostTypes[trimmed]) hostTypes[trimmed] = (dataType as PipelineDataType) ?? 'Generic';
    });
  }

  const outputs = hostOutputs.size ? Array.from(hostOutputs) : fallbackOutputs;
  return { outputs, types: hostTypes };
}

function dedupePipelineSummaries(list: PipelineGraphSummary[]): PipelineGraphSummary[] {
  const out: PipelineGraphSummary[] = [];
  const seen = new Set<string>();
  for (const entry of list) {
    const id = String(entry?.id ?? '').trim();
    if (!id.length || seen.has(id)) continue;
    seen.add(id);
    out.push(entry.id !== id ? { ...entry, id } : entry);
  }
  return out;
}

function dedupeTemplateSummaries(list: PipelineTemplateEntry[]): PipelineTemplateEntry[] {
  const out: PipelineTemplateEntry[] = [];
  const seen = new Set<string>();
  for (const entry of list) {
    const templateId = String(entry?.templateId ?? '').trim();
    const name = String(entry?.name ?? '').trim();
    if (!templateId.length || !name.length || seen.has(templateId)) continue;
    seen.add(templateId);
    out.push({ templateId, name, summary: entry.summary ?? null });
  }
  return out;
}

function normalizeTemplateSummaries(raw: unknown): PipelineTemplateEntry[] {
  if (!Array.isArray(raw)) return [];
  const normalized: PipelineTemplateEntry[] = [];
  for (const entry of raw) {
    if (!entry || typeof entry !== 'object') continue;
    const record = entry as Record<string, unknown>;
    const templateId =
      typeof record.templateId === 'string'
        ? record.templateId
        : typeof record.template_id === 'string'
          ? record.template_id
          : '';
    const name = typeof record.name === 'string' ? record.name : '';
    const summary = typeof record.summary === 'string' || record.summary == null ? (record.summary as string | null) : null;
    normalized.push({ templateId, name, summary });
  }
  return dedupeTemplateSummaries(normalized);
}

function normalizePipelineSummaries(raw: unknown): PipelineGraphSummary[] {
  if (Array.isArray(raw)) {
    return dedupePipelineSummaries(
      raw
        .map<PipelineGraphSummary | null>((entry) => {
          const record = asRecord(entry);
          if (!record) return null;
          const id = asTrimmedString(record.id);
          if (!id.length) return null;
          const name = typeof record.name === 'string' ? record.name.trim() : null;
          const issueCount =
            typeof record.issue_count === 'number' && Number.isFinite(record.issue_count) ? record.issue_count : undefined;
          const updatedAt =
            typeof record.updated_at_ms === 'number' && Number.isFinite(record.updated_at_ms)
              ? record.updated_at_ms
              : undefined;
          return { ...record, id, name, issue_count: issueCount, updated_at_ms: updatedAt } satisfies PipelineGraphSummary;
        })
        .filter((entry): entry is PipelineGraphSummary => Boolean(entry))
    );
  }
  if (raw && typeof raw === 'object') {
    const record = raw as { items?: unknown; graphs?: unknown; pipelines?: unknown };
    if (Array.isArray(record.items)) return dedupePipelineSummaries(record.items as PipelineGraphSummary[]);
    if (Array.isArray(record.graphs)) return dedupePipelineSummaries(record.graphs as PipelineGraphSummary[]);
    if (Array.isArray(record.pipelines)) return dedupePipelineSummaries(record.pipelines as PipelineGraphSummary[]);
  }
  return [];
}

export function createPipelineLayoutData(state: PipelineLayoutState, deps: PipelineLayoutDeps) {
  const apiOptions = { baseUrl: deps.apiBase };
  const pipelineOutputsLoadPromises = new Map<string, Promise<void>>();

  function nextTemplatePipelineName(baseName: string): string {
    const normalizedBase = baseName.trim();
    if (!normalizedBase.length) return 'Template pipeline';
    const taken = new Set(
      state.pipelineGraphs
        .map((entry) => (typeof entry?.name === 'string' ? entry.name.trim().toLowerCase() : ''))
        .filter((value) => value.length)
    );
    if (!taken.has(normalizedBase.toLowerCase())) return normalizedBase;
    let suffix = 2;
    while (taken.has(`${normalizedBase} ${suffix}`.toLowerCase())) {
      suffix += 1;
    }
    return `${normalizedBase} ${suffix}`;
  }

  async function ensurePipelineRegistry(): Promise<DaedalusRegistryResponse | null> {
    if (state.pipelineRegistrySnapshot) return state.pipelineRegistrySnapshot;
    try {
      const registry = await deps.pipelinesApi.listRegistry(apiOptions).catch(() => null);
      state.pipelineRegistrySnapshot = registry ?? null;
    } catch {
      state.pipelineRegistrySnapshot = null;
    }
    return state.pipelineRegistrySnapshot;
  }

  async function ensurePipelineGraphAndOutputs(pipelineId: string, forceRefresh = false): Promise<PipelineGraphAndOutputs> {
    if (pipelineId === RAW_PIPELINE_ID) {
      return { graphJson: RAW_LOOPBACK_GRAPH, filtered: ['raw', 'undistorted'], types: { raw: 'image', undistorted: 'image' } };
    }

    const cachedGraph = state.pipelineGraphCache[pipelineId];
    const hasCachedGraph = cachedGraph != null;
    const manifestGraph = (() => {
      const manifest = asRecord(state.manifestState);
      if (!manifest) return null;
      const normalized = String(pipelineId ?? '').trim();
      if (!normalized.length) return null;
      const activeId = asTrimmedString(manifest.active_pipeline_id);
      if (activeId && activeId === normalized) return null;
      const bindings = Array.isArray(manifest.pipelines) ? manifest.pipelines : [];
      for (const entry of bindings) {
        const binding = asRecord(entry);
        if (!binding) continue;
        const entryId = asTrimmedString(binding.pipeline_id);
        if (entryId && entryId === normalized) {
          return binding.pipeline_graph ?? null;
        }
      }
      return null;
    })();

    let graphJson = !forceRefresh && hasCachedGraph ? cachedGraph : manifestGraph ?? null;
    if (forceRefresh || (!graphJson && !hasCachedGraph)) {
      try {
        const fetched = (await deps.pipelinesApi.fetchGraph({ id: pipelineId }, apiOptions)).graph ?? null;
        if (fetched) graphJson = fetched;
      } catch {
        if (!graphJson && !hasCachedGraph) graphJson = null;
      }
    }

    graphJson = normalizeGraphDocument(graphJson);
    if ((forceRefresh || !hasCachedGraph) && graphJson) {
      state.pipelineGraphCache = { ...state.pipelineGraphCache, [pipelineId]: graphJson };
    }

    const rawOutputs = extractGraphOutputPortsFn(graphJson);
    const portTypes = extractGraphOutputPortTypes(graphJson);
    let planOutputs: string[] = [];
    let planTypes: Record<string, PipelineDataType> = {};

    if (graphJson) {
      try {
        const plan = coercePlanFromGraph(graphJson);
        if (!plan) throw new Error('Invalid plan');
        const derived = derivePlanOutputs(plan, rawOutputs);
        planOutputs = derived.outputs;
        planTypes = derived.types;
      } catch {
        planOutputs = [];
        planTypes = {};
      }
    }

    const candidates = rawOutputs.length ? rawOutputs : planOutputs;
    let resolvedTypes: Record<string, PipelineDataType | null | undefined> = { ...portTypes, ...planTypes };
    let filtered = filterEncoderCompatibleOutputs(candidates, resolvedTypes);
    if (!filtered.length && candidates.length) filtered = candidates;

    if (!filtered.length && pipelineId !== RAW_PIPELINE_ID) {
      try {
        const doc = await deps.pipelinesApi.fetchGraph({ id: pipelineId }, apiOptions);
        const docGraph = normalizeGraphDocument(doc.graph ?? null);
        if (docGraph) {
          const docRawOutputs = extractGraphOutputPortsFn(docGraph);
          const docPortTypes = extractGraphOutputPortTypes(docGraph);
          const plan = coercePlanFromGraph(docGraph);
          if (!plan) throw new Error('Invalid plan');
          const derived = derivePlanOutputs(plan, docRawOutputs);
          const docResolvedTypes: Record<string, PipelineDataType | null | undefined> = {
            ...docPortTypes,
            ...derived.types
          };
          resolvedTypes = { ...resolvedTypes, ...docResolvedTypes };
          filtered = filterEncoderCompatibleOutputs(derived.outputs, docResolvedTypes);
          if (!filtered.length && derived.outputs.length) filtered = derived.outputs;
        }
      } catch {
        // ignore fallback errors
      }
    }

    if (!filtered.length && !candidates.length && pipelineId !== RAW_PIPELINE_ID) {
      const manifest = asRecord(state.manifestState);
      const activeId = asTrimmedString(manifest?.active_pipeline_id);
      const normalizedId = String(pipelineId ?? '').trim();
      const streamId = state.stream?.id ?? state.streamId;
      const matchesActive = normalizedId && normalizedId === activeId;
      if (streamId && matchesActive) {
        try {
          const outputs = await deps.streamsApi.listPipelineOutputs({ id: streamId }, apiOptions);
          const normalizedOutputs = Array.isArray(outputs)
            ? outputs
                .map((value) => {
                  if (!value) return null;
                  if (typeof value === 'string') return String(value).trim();
                  const raw = asRecord(value);
                  if (!raw) return null;
                  const name = typeof raw.name === 'string' ? raw.name.trim() : '';
                  return name && Boolean(raw.previewable) ? name : null;
                })
                .filter((value): value is string => Boolean(value))
            : [];
          if (normalizedOutputs.length) filtered = normalizedOutputs;
        } catch {
          // ignore stream output fallback errors
        }
      }
    }

    if (!filtered.length && candidates.length) filtered = candidates;

    const current = state.pipelineOutputByPipelineId[pipelineId];
    if (current && !filtered.includes(current)) {
      state.pipelineOutputByPipelineId = { ...state.pipelineOutputByPipelineId, [pipelineId]: null };
    } else if (!current && filtered.length) {
      state.pipelineOutputByPipelineId = { ...state.pipelineOutputByPipelineId, [pipelineId]: filtered[0] };
    }

    const cachedOutputs = state.pipelineOutputOptionsCache[pipelineId];
    if (!stringArraysEqual(cachedOutputs, filtered)) {
      state.pipelineOutputOptionsCache = { ...state.pipelineOutputOptionsCache, [pipelineId]: filtered };
    }

    return { graphJson, filtered, types: resolvedTypes };
  }

  async function ensurePipelineOutputsLoaded(pipelineId: string): Promise<void> {
    const normalized = String(pipelineId ?? '').trim();
    if (!normalized.length) return;
    if (Object.prototype.hasOwnProperty.call(state.pipelineOutputOptionsCache, normalized)) return;
    const inFlight = pipelineOutputsLoadPromises.get(normalized);
    if (inFlight) {
      await inFlight;
      return;
    }
    const loadPromise = (async () => {
      try {
        await ensurePipelineGraphAndOutputs(normalized);
      } catch (err) {
        console.warn('Failed to load pipeline outputs', err);
      } finally {
        pipelineOutputsLoadPromises.delete(normalized);
      }
    })();
    pipelineOutputsLoadPromises.set(normalized, loadPromise);
    await loadPromise;
  }

  async function refreshPipelineGraphs(): Promise<PipelineGraphSummary[]> {
    if (state.pipelineGraphLoading) return state.pipelineGraphs;
    state.pipelineGraphLoading = true;
    state.pipelineGraphError = null;
    try {
      const graphs = await deps.pipelinesApi.listGraphs(apiOptions);
      const normalized = normalizePipelineSummaries(graphs);
      if (!normalized.length && state.pipelineGraphs.length) return state.pipelineGraphs;
      state.pipelineGraphs = normalized;
      return normalized;
    } catch (err) {
      console.warn('Failed to load pipeline graphs', err);
      state.pipelineGraphError = 'Unable to load pipelines';
      state.pipelineGraphs = [];
      return [];
    } finally {
      state.pipelineGraphLoading = false;
    }
  }

  async function refreshPipelineOutputs(pipelineId: string | null): Promise<void> {
    if (!pipelineId) {
      state.pipelineOutputOptions = [];
      state.selectedPipelineOutput = null;
      state.selectedPipelineGraph = null;
      return;
    }
    try {
      const { graphJson, filtered } = await ensurePipelineGraphAndOutputs(pipelineId);
      state.selectedPipelineGraph = graphJson;
      state.pipelineOutputOptions = filtered;
      if (state.selectedPipelineOutput && filtered.includes(state.selectedPipelineOutput)) return;
      state.selectedPipelineOutput = filtered[0] ?? null;
    } catch (err) {
      console.warn('Failed to load pipeline graph', err);
      state.pipelineOutputOptions = [];
      state.selectedPipelineOutput = null;
      state.selectedPipelineGraph = null;
    }
  }

  async function listPipelineTemplatesForAssign(): Promise<PipelineTemplateEntry[]> {
    const templates = await deps.pipelinesApi.listTemplates(apiOptions);
    return normalizeTemplateSummaries(templates);
  }

  async function createPipelineFromTemplateAndAssign(templateId: string): Promise<PipelineTemplateCreateResult> {
    const normalizedTemplateId = String(templateId ?? '').trim();
    if (!normalizedTemplateId.length) {
      throw new Error('Select a template first.');
    }
    try {
      const template: PipelineTemplateDocument = await deps.pipelinesApi.fetchTemplate({ id: normalizedTemplateId }, apiOptions);
      const graph = normalizeGraphDocument(template.graph ?? null);
      if (!graph || typeof graph !== 'object') {
        throw new Error('Template graph is empty.');
      }
      const baseName =
        typeof template.name === 'string' && template.name.trim().length ? template.name.trim() : `Template ${normalizedTemplateId}`;
      const uploadName = nextTemplatePipelineName(baseName);
      const created: PipelineDocument = await deps.pipelinesApi.uploadGraph({
        requestBody: {
          graph,
          name: uploadName
        }
      }, apiOptions);
      const createdId = String(created.id ?? '').trim();
      if (!createdId.length) {
        throw new Error('Created pipeline did not return an id.');
      }
      const createdName =
        typeof created.name === 'string' && created.name.trim().length ? created.name.trim() : uploadName;

      state.pipelineAssignDraft = normalizeAssignedPipelineIds([...state.pipelineAssignDraft, createdId]);
      const refreshed = await refreshPipelineGraphs();
      if (!refreshed.some((entry) => String(entry?.id ?? '').trim() === createdId)) {
        state.pipelineGraphs = dedupePipelineSummaries([
          { id: createdId, name: createdName, updated_at_ms: created.updated_at_ms },
          ...state.pipelineGraphs
        ]);
      }
      return { id: createdId, name: createdName };
    } catch (error) {
      const normalizedError = error instanceof Error ? error : new Error('Unable to create pipeline from template.');
      deps.reportError({
        title: 'Pipeline template failed',
        error: normalizedError,
        fallback: 'Unable to create a pipeline from this template right now.'
      });
      throw normalizedError;
    }
  }

  return {
    ensurePipelineRegistry,
    ensurePipelineGraphAndOutputs,
    ensurePipelineOutputsLoaded,
    refreshPipelineGraphs,
    refreshPipelineOutputs,
    listPipelineTemplatesForAssign,
    createPipelineFromTemplateAndAssign
  };
}
