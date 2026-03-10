import type { StreamInfo, StreamManifest } from '$lib/api/httpClient';
import { apiFetchResponse } from '$lib/api/core/http';
import { OpenAPI } from '$lib/ts-bindings/http/client';
import { getHttpClientBase } from '$lib/api/httpClient';
import type { PipelinesApi } from '$lib/api/pipelinesApi';
import type { StreamsApi } from '$lib/api/streamsApi';
import type {
  DaedalusRegistryResponse,
  PipelineDocument,
  PipelineTemplateDocument,
  StreamPipelineWire
} from '$lib/ts-bindings/http/client';
import type { PipelineDataType, PipelineGraphPlan } from '$lib/types/pipeline';
import { normalizeGridSlots, normalizeGridOutputKeys } from './cameraPipelineState';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  RAW_LOOPBACK_GRAPH,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID,
  normalizeAssignedPipelineIds
} from './cameraPipelineTuningController';
import {
  hydratePipelineUi as hydratePipelineUiState,
  persistPipelineUi as persistPipelineUiState,
  pipelineUiStorageKey as pipelineUiStorageKeyState
} from './layoutPersistence';
import {
  buildPipelineLayoutPayload as buildPipelineLayoutPayloadFn,
  ensureAssignedPipelineId,
  extractGraphOutputPorts as extractGraphOutputPortsFn,
  gridHasUnappliedPipelines as gridHasUnappliedPipelinesFn,
  isMultiplexLayout as isMultiplexLayoutFn,
  isPipelineApplied as isPipelineAppliedFn,
  outputSelectionForPipeline as outputSelectionForPipelineFn,
  pipelineLayoutSignature as pipelineLayoutSignatureFn,
  type PipelineLayoutPayload
} from './layoutValidation';
import { collectPipelineOutputs } from '$lib/features/pipelines/boundaryOutputs';
import { normalizeDaedalusRegistry } from '$lib/features/pipelines/controller/daedalusRegistry';
import { fromApiGraphPlan } from '$lib/features/pipelines/graphConverters';
import { hydrateGraphWithRegistry } from '$lib/features/pipelines/styleHydration';
import { extractGraphOutputPortTypes, filterEncoderCompatibleOutputs } from '$lib/features/pipelines/outputFilters';
import {
  gridKey as gridKeyFn,
  outputKeyForCell as outputKeyForCellFn,
  pipelineForCell as pipelineForCellFn,
  setOutputKeyForCell as setOutputKeyForCellFn,
  setPipelineForCell as setPipelineForCellFn
} from './gridMapping';

type PipelineLayoutState = {
  get stream(): StreamInfo | null;
  get streamId(): string;
  get manifestState(): StreamManifest | null;
  get pipelineRegistrySnapshot(): DaedalusRegistryResponse | null;
  set pipelineRegistrySnapshot(value: DaedalusRegistryResponse | null);
  get pipelineGraphCache(): Record<string, unknown>;
  set pipelineGraphCache(value: Record<string, unknown>);
  get pipelineOutputOptionsCache(): Record<string, string[]>;
  set pipelineOutputOptionsCache(value: Record<string, string[]>);
  get pipelineOutputByPipelineId(): Record<string, string | null>;
  set pipelineOutputByPipelineId(value: Record<string, string | null>);
  get pipelineGraphLoading(): boolean;
  set pipelineGraphLoading(value: boolean);
  get pipelineGraphError(): string | null;
  set pipelineGraphError(value: string | null);
  get pipelineGraphs(): PipelineGraphSummary[];
  set pipelineGraphs(value: PipelineGraphSummary[]);
  get pipelineOutputOptions(): string[];
  set pipelineOutputOptions(value: string[]);
  get selectedPipelineOutput(): string | null;
  set selectedPipelineOutput(value: string | null);
  get selectedPipelineGraph(): unknown;
  set selectedPipelineGraph(value: unknown);
  get pipelineLayoutApplyTimer(): number | null;
  set pipelineLayoutApplyTimer(value: number | null);
  get pipelineLayoutTouched(): boolean;
  set pipelineLayoutTouched(value: boolean);
  get pipelineAssignmentsTouched(): boolean;
  set pipelineAssignmentsTouched(value: boolean);
  get pipelineGridRows(): number;
  set pipelineGridRows(value: number);
  get pipelineGridColumns(): number;
  set pipelineGridColumns(value: number);
  get pipelineGridSlots(): Record<string, string | null>;
  set pipelineGridSlots(value: Record<string, string | null>);
  get pipelineGridSlotOutputKeys(): Record<string, string | null>;
  set pipelineGridSlotOutputKeys(value: Record<string, string | null>);
  get assignedPipelineIds(): string[];
  set assignedPipelineIds(value: string[]);
  get pipelineAssignDraft(): string[];
  set pipelineAssignDraft(value: string[]);
  get pipelineAssignQuery(): string;
  set pipelineAssignQuery(value: string);
  get pipelineAssignModalOpen(): boolean;
  set pipelineAssignModalOpen(value: boolean);
  get pipelineRemoveModalOpen(): boolean;
  set pipelineRemoveModalOpen(value: boolean);
  get pipelineRemoveCandidateId(): string | null;
  set pipelineRemoveCandidateId(value: string | null);
  get pipelineDragPayload(): { pipelineId: string; from?: { row: number; column: number } } | null;
  set pipelineDragPayload(value: { pipelineId: string; from?: { row: number; column: number } } | null);
  get selectedPipelineId(): string | null;
  set selectedPipelineId(value: string | null);
  get pipelineUiHydrated(): boolean;
};

type PipelineLayoutDeps = {
  pipelinesApi: typeof PipelinesApi;
  streamsApi: typeof StreamsApi;
  reportError: (args: { title: string; error: unknown; fallback: string }) => void;
  refresh: () => Promise<void>;
  scheduleStreamPresetApply: () => void;
  onExternalLayoutApplied?: () => void;
  applyPipelineOverridesToGraph: (pipelineId: string, graph: unknown) => unknown;
  apiPath: (path: string) => string;
  apiBase?: string;
  layoutDebounceMs: number;
  storagePrefix: string;
};

type PipelineGraphSummary = Record<string, unknown> & {
  id: string;
  name?: string | null;
  issue_count?: number;
  updated_at_ms?: number;
};

type PipelineTemplateEntry = {
  templateId: string;
  name: string;
  summary?: string | null;
};

export function createPipelineLayoutController(state: PipelineLayoutState, deps: PipelineLayoutDeps) {
  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === 'object' ? (value as Record<string, unknown>) : null;
  const asTrimmedString = (value: unknown): string =>
    typeof value === 'string' ? value.trim() : '';
  const normalizeGraphDocument = (value: unknown): unknown => {
    const record = asRecord(value);
    if (!record) return value;
    const nested = asRecord(record.graph) ?? asRecord(record.pipeline_graph) ?? asRecord(record.pipelineGraph);
    if (nested) {
      const baseMetadata = asRecord(record.metadata);
      const nestedMetadata = asRecord(nested.metadata);
      if (baseMetadata && !nestedMetadata) {
        return { ...nested, metadata: baseMetadata };
      }
      if (baseMetadata && nestedMetadata && typeof baseMetadata === 'object' && typeof nestedMetadata === 'object') {
        return { ...nested, metadata: { ...baseMetadata, ...nestedMetadata } };
      }
      return nested;
    }
    return value;
  };
  const coercePlanFromGraph = (graph: unknown): PipelineGraphPlan | null => {
    const record = asRecord(graph);
    if (!record) return null;
    const nodes = record.nodes;
    const connections = record.connections;
    if (nodes && typeof nodes === 'object' && !Array.isArray(nodes) && Array.isArray(connections)) {
      return graph as PipelineGraphPlan;
    }
    try {
      return fromApiGraphPlan(graph as Parameters<typeof fromApiGraphPlan>[0]);
    } catch {
      return null;
    }
  };

  const derivePlanOutputs = (
    plan: PipelineGraphPlan | null,
    fallbackOutputs: string[]
  ): { outputs: string[]; types: Record<string, PipelineDataType> } => {
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
  };

  function ensureApiBase(): void {
    try {
      getHttpClientBase();
      return;
    } catch {
      // fall back to legacy injected base when local storage/env resolution fails
    }
    if (!deps.apiBase) return;
    OpenAPI.BASE = deps.apiBase.replace(/\/+$/, '');
  }

  async function ensurePipelineRegistry(): Promise<DaedalusRegistryResponse | null> {
    if (state.pipelineRegistrySnapshot) return state.pipelineRegistrySnapshot;
    try {
      ensureApiBase();
      const registry = await deps.pipelinesApi.listRegistry().catch(() => null);
      state.pipelineRegistrySnapshot = registry ?? null;
    } catch {
      state.pipelineRegistrySnapshot = null;
    }
    return state.pipelineRegistrySnapshot;
  }

  async function ensurePipelineGraphAndOutputs(
    pipelineId: string,
    forceRefresh = false
  ): Promise<{
    graphJson: unknown;
    filtered: string[];
    types: Record<string, PipelineDataType | null | undefined>;
    }> {
    if (pipelineId === RAW_PIPELINE_ID) {
      // Treat the RAW stream pipeline as having two "user-facing" outputs: raw + undistorted.
      // (The underlying graph may still expose `frame` as an alias, but we don't surface it.)
      const filtered = ['raw', 'undistorted'];
      return { graphJson: RAW_LOOPBACK_GRAPH, filtered, types: { raw: 'image', undistorted: 'image' } };
    }

    ensureApiBase();
    const cachedGraph = state.pipelineGraphCache[pipelineId];
    const hasCachedGraph = cachedGraph != null;
    const manifestGraph = (() => {
      const manifest = asRecord(state.manifestState);
      if (!manifest) return null;
      const normalized = String(pipelineId ?? '').trim();
      if (!normalized.length) return null;
      const activeId = asTrimmedString(manifest.active_pipeline_id);
      const legacyId = asTrimmedString(manifest.pipeline_id);
      if ((activeId && activeId === normalized) || (legacyId && legacyId === normalized)) {
        return manifest.pipeline_graph ?? manifest.pipelineGraph ?? manifest.graph ?? null;
      }
      const bindings = Array.isArray(manifest.pipelines) ? manifest.pipelines : [];
      for (const entry of bindings) {
        const binding = asRecord(entry);
        if (!binding) continue;
        const raw =
          asTrimmedString(binding.pipeline_id) ||
          asTrimmedString(binding.pipelineId) ||
          asTrimmedString(binding.id);
        const entryId = raw.trim();
        if (entryId && entryId === normalized) {
          return binding.pipeline_graph ?? binding.pipelineGraph ?? binding.graph ?? null;
        }
      }
      return null;
    })();
    let graphJson = !forceRefresh && hasCachedGraph ? cachedGraph : manifestGraph ?? null;
    if (forceRefresh || (!graphJson && !hasCachedGraph)) {
      try {
        const fetched = (await deps.pipelinesApi.fetchGraph({ id: pipelineId })).graph ?? null;
        if (fetched) {
          graphJson = fetched;
        }
      } catch {
        if (!graphJson && !hasCachedGraph) {
          graphJson = null;
        }
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
    const registrySnapshot = await ensurePipelineRegistry();
    const registryEntries =
      registrySnapshot && Array.isArray(registrySnapshot.nodes)
        ? normalizeDaedalusRegistry(registrySnapshot.nodes, registrySnapshot.types ?? undefined)
        : [];
    if (graphJson) {
      try {
        const plan = coercePlanFromGraph(graphJson);
        if (!plan) throw new Error('Invalid plan');
        hydrateGraphWithRegistry(plan, registryEntries);
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
    if (!filtered.length && pipelineId !== RAW_PIPELINE_ID) {
      try {
        const doc = await deps.pipelinesApi.fetchGraph({ id: pipelineId });
        const docGraph = normalizeGraphDocument(doc.graph ?? null);
        if (docGraph) {
          const docRawOutputs = extractGraphOutputPortsFn(docGraph);
          const docPortTypes = extractGraphOutputPortTypes(docGraph);
          const plan = coercePlanFromGraph(docGraph);
          if (!plan) throw new Error('Invalid plan');
          hydrateGraphWithRegistry(plan, registryEntries);
          const derived = derivePlanOutputs(plan, docRawOutputs);
          const docOutputs = derived.outputs;
          const docTypes = derived.types;
          const docResolvedTypes: Record<string, PipelineDataType | null | undefined> = { ...docPortTypes, ...docTypes };
          resolvedTypes = { ...resolvedTypes, ...docResolvedTypes };
          filtered = filterEncoderCompatibleOutputs(docOutputs, docResolvedTypes);
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
          const outputs = await deps.streamsApi.listPipelineOutputs({ id: streamId });
          const normalizedOutputs = Array.isArray(outputs)
            ? outputs
                .map((value) => {
                  if (!value) return null;
                  if (typeof value === 'string') return String(value).trim();
                  const raw = asRecord(value);
                  if (!raw) return null;
                  // New descriptor shape: only allow previewable/image-like ports.
                  const name = typeof raw.name === 'string' ? raw.name.trim() : '';
                  if (!name) return null;
                  const previewable = Boolean(raw.previewable);
                  return previewable ? name : null;
                })
                .filter((value): value is string => Boolean(value))
            : [];
          if (normalizedOutputs.length) {
            filtered = normalizedOutputs;
          }
        } catch {
          // ignore stream output fallback errors
        }
      }
    }
    if (!filtered.length && candidates.length) {
      filtered = candidates;
    }
    const current = state.pipelineOutputByPipelineId[pipelineId];
    if (current && !filtered.includes(current)) {
      state.pipelineOutputByPipelineId = { ...state.pipelineOutputByPipelineId, [pipelineId]: null };
    } else if (!current && filtered.length) {
      state.pipelineOutputByPipelineId = { ...state.pipelineOutputByPipelineId, [pipelineId]: filtered[0] };
    }
    state.pipelineOutputOptionsCache = { ...state.pipelineOutputOptionsCache, [pipelineId]: filtered };
    return { graphJson, filtered, types: resolvedTypes };
  }

  async function ensurePipelineOutputsLoaded(pipelineId: string): Promise<void> {
    const normalized = String(pipelineId ?? '').trim();
    if (!normalized.length) return;
    try {
      await ensurePipelineGraphAndOutputs(normalized);
    } catch (err) {
      console.warn('Failed to load pipeline outputs', err);
    }
  }

  function extractGraphOutputPorts(graph: unknown): string[] {
    return extractGraphOutputPortsFn(graph);
  }

  function outputSelectionForPipeline(pipelineId: string): string | null {
    return outputSelectionForPipelineFn(state, pipelineId);
  }

  function dedupePipelineSummaries(list: PipelineGraphSummary[]): PipelineGraphSummary[] {
    const out: PipelineGraphSummary[] = [];
    const seen = new Set<string>();
    for (const entry of list) {
      const id = String(entry?.id ?? '').trim();
      if (!id.length) continue;
      if (seen.has(id)) continue;
      seen.add(id);
      if (entry.id !== id) {
        out.push({ ...entry, id });
      } else {
        out.push(entry);
      }
    }
    return out;
  }

  function dedupeTemplateSummaries(list: PipelineTemplateEntry[]): PipelineTemplateEntry[] {
    const out: PipelineTemplateEntry[] = [];
    const seen = new Set<string>();
    for (const entry of list) {
      const templateId = String(entry?.templateId ?? '').trim();
      const name = String(entry?.name ?? '').trim();
      if (!templateId.length || !name.length) continue;
      if (seen.has(templateId)) continue;
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
            return {
              ...record,
              id,
              name,
              issue_count: issueCount,
              updated_at_ms: updatedAt
            } satisfies PipelineGraphSummary;
          })
          .filter((entry): entry is PipelineGraphSummary => Boolean(entry))
      );
    }
    if (raw && typeof raw === 'object') {
      const record = raw as { items?: unknown; graphs?: unknown; pipelines?: unknown };
      if (Array.isArray(record.items)) return dedupePipelineSummaries(record.items);
      if (Array.isArray(record.graphs)) return dedupePipelineSummaries(record.graphs);
      if (Array.isArray(record.pipelines)) return dedupePipelineSummaries(record.pipelines);
    }
    return [];
  }

  function isMultiplexLayout(rows = state.pipelineGridRows, columns = state.pipelineGridColumns): boolean {
    return isMultiplexLayoutFn(rows, columns);
  }

  function isPipelineApplied(pipelineId: string | null): boolean {
    return isPipelineAppliedFn(state, pipelineId);
  }

  function gridHasUnappliedPipelines(): boolean {
    return gridHasUnappliedPipelinesFn(state);
  }

  function buildPipelineLayoutPayload(): PipelineLayoutPayload | null {
    return buildPipelineLayoutPayloadFn(state);
  }

  async function applyPipelineGraphForId(pipelineId: string): Promise<boolean> {
    if (!state.stream?.id) return false;
    const normalized = String(pipelineId ?? '').trim();
    if (!normalized.length) return false;
    ensureApiBase();
    try {
      const output = outputSelectionForPipelineFn(state, normalized);
      if (normalized === RAW_PIPELINE_ID) {
        await deps.streamsApi.setPipelineGraph({
          id: state.stream.id,
          requestBody: { graph: RAW_LOOPBACK_GRAPH, pipeline_id: RAW_PIPELINE_UUID, output: output ?? null }
        });
      } else {
        const doc = await deps.pipelinesApi.fetchGraph({ id: normalized });
        const graph = doc.graph ?? null;
        if (!graph) {
          throw new Error(`Pipeline graph missing: ${normalized}`);
        }
        const graphWithOverrides = deps.applyPipelineOverridesToGraph(normalized, graph);
        await deps.streamsApi.setPipelineGraph({
          id: state.stream.id,
          requestBody: { graph: graphWithOverrides, pipeline_id: normalized, output: output ?? null }
        });
      }
      await deps.refresh();
      return true;
    } catch (err) {
      console.warn('Failed to apply pipeline graph', normalized, err);
      deps.reportError({
        title: 'Pipeline apply failed',
        error: err,
        fallback: 'Unable to apply pipeline changes right now.'
      });
      return false;
    }
  }

  async function applyUnappliedPipelines(): Promise<boolean> {
    const missing = new Set<string>();
    for (const pipelineId of Object.values(state.pipelineGridSlots)) {
      const normalized = typeof pipelineId === 'string' ? pipelineId.trim() : '';
      if (!normalized.length) continue;
      if (!isPipelineAppliedFn(state, normalized)) {
        missing.add(normalized);
      }
    }
    for (const pipelineId of missing) {
      const ok = await applyPipelineGraphForId(pipelineId);
      if (!ok) return false;
    }
    return true;
  }

  // Note: we intentionally do not auto-collapse multiplex grids back to 1x1 raw. In multiplex,
  // empty cells should render as empty (black) until the user explicitly places a pipeline (raw
  // or otherwise) into a cell.

  function schedulePipelineLayoutApply(): void {
    if (!state.stream?.id) return;
    const rows = Math.min(Math.max(Math.trunc(state.pipelineGridRows), 1), 6);
    const columns = Math.min(Math.max(Math.trunc(state.pipelineGridColumns), 1), 6);
    const multiplex = isMultiplexLayoutFn(rows, columns);
    let nextLayout: PipelineLayoutPayload | null = null;

    if (!multiplex) {
      const normalizedSlots = normalizeGridSlots(rows, columns, state.pipelineGridSlots);
      const normalizedOutputs = normalizeGridOutputKeys(rows, columns, state.pipelineGridSlotOutputKeys);
      state.pipelineGridSlots = normalizedSlots;
      state.pipelineGridSlotOutputKeys = normalizedOutputs;
      const outputCellPipeline = normalizedSlots[PIPELINE_OUTPUT_CELL_KEY];
      const hasAssignedPipeline = typeof outputCellPipeline === 'string' && outputCellPipeline.trim().length > 0;
      if (hasAssignedPipeline) {
        const normalizedId = outputCellPipeline.trim();
        let output_key = normalizedOutputs[PIPELINE_OUTPUT_CELL_KEY] ?? outputSelectionForPipelineFn(state, normalizedId);
        if (normalizedId === RAW_PIPELINE_ID && typeof output_key === 'string' && output_key.trim().toLowerCase() === 'frame') {
          output_key = 'raw';
        }
        nextLayout = {
          rows,
          columns,
          slots: [
            {
              row: 0,
              column: 0,
              pipeline_id: normalizedId === RAW_PIPELINE_ID ? RAW_PIPELINE_UUID : normalizedId,
              output_key: typeof output_key === 'string' && output_key.trim().length ? output_key.trim() : null
            }
          ]
        };
      } else {
        // Keep clear behavior consistent: even in 1x1, an explicitly empty grid should stay blank.
        nextLayout = { rows, columns, slots: [] };
      }
    } else {
      nextLayout = buildPipelineLayoutPayloadFn(state);
      if (!nextLayout) return;
    }

    if (state.pipelineLayoutApplyTimer != null) {
      clearTimeout(state.pipelineLayoutApplyTimer);
    }
    state.pipelineLayoutApplyTimer = window.setTimeout(async () => {
      state.pipelineLayoutApplyTimer = null;
      if (gridHasUnappliedPipelinesFn(state)) {
        const applied = await applyUnappliedPipelines();
        if (!applied) return;
      }
      try {
        await deps.streamsApi.setPipelineLayout({ id: state.stream!.id, requestBody: { pipeline_layout: nextLayout } });
        await deps.refresh();
        deps.onExternalLayoutApplied?.();
      } catch (err) {
        console.warn('Failed to update pipeline layout', err);
        deps.reportError({
          title: 'Pipeline layout failed',
          error: err,
          fallback: 'Unable to update the pipeline layout right now.'
        });
        await deps.refresh();
      }
    }, deps.layoutDebounceMs);
  }

  function setOutputSelectionForPipeline(pipelineId: string, output: string | null): void {
    const normalizedId = String(pipelineId ?? '').trim();
    if (!normalizedId.length) return;
    let normalizedOutput = typeof output === 'string' && output.trim().length ? output.trim() : null;
    if (normalizedId === RAW_PIPELINE_ID && normalizedOutput && normalizedOutput.toLowerCase() === 'frame') {
      normalizedOutput = 'raw';
    }
    const prevRaw = state.pipelineOutputByPipelineId[normalizedId];
    const prev = typeof prevRaw === 'string' && prevRaw.trim().length ? prevRaw.trim() : null;
    if (prev === normalizedOutput) return;
    state.pipelineOutputByPipelineId = { ...state.pipelineOutputByPipelineId, [normalizedId]: normalizedOutput };
  }

  async function refreshPipelineGraphs(): Promise<PipelineGraphSummary[]> {
    if (state.pipelineGraphLoading) return state.pipelineGraphs;
    state.pipelineGraphLoading = true;
    state.pipelineGraphError = null;
    try {
      ensureApiBase();
      const graphs = await deps.pipelinesApi.listGraphs();
      const normalized = normalizePipelineSummaries(graphs);
      if (!normalized.length && state.pipelineGraphs.length) {
        return state.pipelineGraphs;
      }
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

  async function listPipelineTemplatesForAssign(): Promise<PipelineTemplateEntry[]> {
    ensureApiBase();
    const templates = await deps.pipelinesApi.listTemplates();
    return normalizeTemplateSummaries(templates);
  }

  async function createPipelineFromTemplateAndAssign(templateId: string): Promise<{ id: string; name: string }> {
    const normalizedTemplateId = String(templateId ?? '').trim();
    if (!normalizedTemplateId.length) {
      throw new Error('Select a template first.');
    }
    ensureApiBase();
    try {
      const template: PipelineTemplateDocument = await deps.pipelinesApi.fetchTemplate({ id: normalizedTemplateId });
      const graph = normalizeGraphDocument(template.graph ?? null);
      if (!graph || typeof graph !== 'object') {
        throw new Error('Template graph is empty.');
      }
      const baseName =
        typeof template.name === 'string' && template.name.trim().length
          ? template.name.trim()
          : `Template ${normalizedTemplateId}`;
      const uploadName = nextTemplatePipelineName(baseName);
      const created: PipelineDocument = await deps.pipelinesApi.uploadGraph({
        requestBody: {
          graph,
          name: uploadName
        }
      });
      const createdId = String(created.id ?? '').trim();
      if (!createdId.length) {
        throw new Error('Created pipeline did not return an id.');
      }
      const createdName =
        typeof created.name === 'string' && created.name.trim().length
          ? created.name.trim()
          : uploadName;

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
      if (state.selectedPipelineOutput && filtered.includes(state.selectedPipelineOutput)) {
        return;
      }
      state.selectedPipelineOutput = filtered[0] ?? null;
    } catch (err) {
      console.warn('Failed to load pipeline graph', err);
      state.pipelineOutputOptions = [];
      state.selectedPipelineOutput = null;
      state.selectedPipelineGraph = null;
    }
  }

  async function setLivePipelineOutput(output: string | null): Promise<void> {
    if (!state.stream?.id) return;
    try {
      const resp = await apiFetchResponse(deps.apiPath(`/streams/${encodeURIComponent(state.stream.id)}/pipeline/output`), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ output })
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Request failed (${resp.status})`);
      }
    } catch (err) {
      console.warn('Failed to set pipeline output', err);
      deps.reportError({
        title: 'Pipeline output failed',
        error: err,
        fallback: 'Unable to update pipeline output right now.'
      });
    }
  }

  function currentPipelineWires(): StreamPipelineWire[] {
    const manifest = asRecord(state.manifestState);
    const wires = manifest?.pipeline_wires ?? manifest?.pipelineWires ?? null;
    return Array.isArray(wires) ? wires : [];
  }

  async function setFrameSourceForPipelineInstance(args: {
    to: { pipelineId: string; outputKey?: string | null };
    from: { pipelineId: string; outputKey?: string | null; port?: string | null } | null;
  }): Promise<void> {
    if (!state.stream?.id) return;
    const toId = String(args.to?.pipelineId ?? '').trim();
    if (!toId.length) return;
    const toUuid = toId === RAW_PIPELINE_ID ? RAW_PIPELINE_UUID : toId;
    const toOutputKey = typeof args.to.outputKey === 'string' && args.to.outputKey.trim().length ? args.to.outputKey.trim() : null;

    const normalizeId = (value: string): string => (value === RAW_PIPELINE_ID ? RAW_PIPELINE_UUID : value);
    const normalizeKey = (value: unknown): string | null => (typeof value === 'string' && value.trim().length ? value.trim() : null);
    const normalizePort = (value: unknown, fallback: string): string => {
      const raw = typeof value === 'string' ? value.trim() : '';
      return raw.length ? raw : fallback;
    };

    const prev = currentPipelineWires();
    const next = prev.filter((wire) => {
      const to = wire?.to ?? null;
      const wireToId = typeof to?.pipeline_id === 'string' ? to.pipeline_id.trim() : '';
      const wireToKey = normalizeKey(to?.output_key);
      const wireToPort = normalizePort(to?.port, 'frame').toLowerCase();
      if (!wireToId.length) return true;
      if (wireToPort !== 'frame') return true;
      if (wireToId !== toUuid) return true;
      // Match instance key (output_key) when provided; otherwise treat null/empty as the default instance.
      if ((wireToKey ?? null) !== (toOutputKey ?? null)) return true;
      return false;
    });

    if (args.from) {
      const fromId = String(args.from.pipelineId ?? '').trim();
      if (fromId.length) {
        const fromUuid = normalizeId(fromId);
        const fromOutputKey = typeof args.from.outputKey === 'string' && args.from.outputKey.trim().length ? args.from.outputKey.trim() : null;
        const fromPort = typeof args.from.port === 'string' && args.from.port.trim().length ? args.from.port.trim() : null;
        next.push({
          from: { pipeline_id: fromUuid, output_key: fromOutputKey, port: fromPort },
          to: { pipeline_id: toUuid, output_key: toOutputKey, port: 'frame' }
        });
      }
    }

    try {
      await deps.streamsApi.setPipelineWires({ id: state.stream.id, requestBody: { wires: next } });
      await deps.refresh();
    } catch (err) {
      console.warn('Failed to set pipeline wires', err);
      deps.reportError({
        title: 'Pipeline wiring failed',
        error: err,
        fallback: 'Unable to update pipeline wiring right now.'
      });
      await deps.refresh();
    }
  }

  function pipelineLabel(pipelineId: string): string {
    if (pipelineId === RAW_PIPELINE_ID) return 'Raw stream';
    const entry = state.pipelineGraphs.find((g) => String(g?.id ?? '') === pipelineId) ?? null;
    const name = entry?.name?.trim?.() ? String(entry.name).trim() : '';
    return name.length ? name : pipelineId;
  }

  function openPipelineAssignModal(): void {
    state.pipelineAssignDraft = normalizeAssignedPipelineIds(state.assignedPipelineIds);
    state.pipelineAssignQuery = '';
    state.pipelineAssignModalOpen = true;
    if (!state.pipelineGraphs.length && !state.pipelineGraphLoading) {
      void refreshPipelineGraphs();
    }
  }

  function closePipelineAssignModal(): void {
    state.pipelineAssignModalOpen = false;
  }

  function savePipelineAssignModal(): void {
    const normalized = normalizeAssignedPipelineIds(state.pipelineAssignDraft);
    applyAssignedPipelineIds(normalized);
    state.pipelineAssignModalOpen = false;
    deps.scheduleStreamPresetApply();
  }

  function dropPipelineEverywhere(pipelineId: string): void {
    const next: Record<string, string | null> = { ...state.pipelineGridSlots };
    Object.keys(next).forEach((key) => {
      if (next[key] === pipelineId) next[key] = null;
    });
    state.pipelineGridSlots = next;
    state.pipelineGridSlotOutputKeys = normalizeGridOutputKeys(state.pipelineGridRows, state.pipelineGridColumns, state.pipelineGridSlotOutputKeys);
    if (state.pipelineOutputByPipelineId[pipelineId] !== undefined) {
      const outputs = { ...state.pipelineOutputByPipelineId };
      delete outputs[pipelineId];
      state.pipelineOutputByPipelineId = outputs;
    }
  }

  function applyAssignedPipelineIds(nextIds: string[]): void {
    const normalized = normalizeAssignedPipelineIds(nextIds);
    if (normalized.join('|') !== normalizeAssignedPipelineIds(state.assignedPipelineIds).join('|')) {
      state.pipelineLayoutTouched = true;
      state.pipelineAssignmentsTouched = true;
    }
    state.assignedPipelineIds = normalized;
    state.pipelineAssignDraft = normalized;
    state.pipelineGridSlots = normalizeGridSlots(state.pipelineGridRows, state.pipelineGridColumns, state.pipelineGridSlots);
    Object.values(state.pipelineGridSlots).forEach((pipelineId) => {
      if (pipelineId && pipelineId !== RAW_PIPELINE_ID && !normalized.includes(pipelineId)) {
        dropPipelineEverywhere(pipelineId);
      }
    });
    state.pipelineOutputByPipelineId = Object.fromEntries(
      Object.entries(state.pipelineOutputByPipelineId).filter(([id]) => normalized.includes(id))
    );
    if (state.selectedPipelineId && !normalized.includes(state.selectedPipelineId)) {
      const stillInGrid = Object.values(state.pipelineGridSlots ?? {}).some((id) => id === state.selectedPipelineId);
      if (!stillInGrid) {
        state.selectedPipelineId = null;
        state.selectedPipelineOutput = null;
      }
    }
  }

  function openPipelineRemoveModal(pipelineId: string): void {
    state.pipelineRemoveCandidateId = pipelineId;
    state.pipelineRemoveModalOpen = true;
  }

  function closePipelineRemoveModal(): void {
    state.pipelineRemoveModalOpen = false;
    state.pipelineRemoveCandidateId = null;
  }

  function confirmPipelineRemove(): void {
    const pipelineId = state.pipelineRemoveCandidateId;
    if (!pipelineId) {
      closePipelineRemoveModal();
      return;
    }
    applyAssignedPipelineIds(state.assignedPipelineIds.filter((id) => id !== pipelineId));
    closePipelineRemoveModal();
    deps.scheduleStreamPresetApply();
  }

  function setPipelineGridDimensions(rows: number, columns: number): void {
    state.pipelineLayoutTouched = true;
    const prevRows = Math.min(Math.max(Math.trunc(state.pipelineGridRows ?? 1), 1), 6);
    const prevColumns = Math.min(Math.max(Math.trunc(state.pipelineGridColumns ?? 1), 1), 6);
    const prevMultiplex = isMultiplexLayoutFn(prevRows, prevColumns);
    const prevSlots = normalizeGridSlots(prevRows, prevColumns, state.pipelineGridSlots);

    const nextRows = Math.min(Math.max(Math.trunc(rows), 1), 6);
    const nextColumns = Math.min(Math.max(Math.trunc(columns), 1), 6);
    const nextMultiplex = isMultiplexLayoutFn(nextRows, nextColumns);
    state.pipelineGridRows = nextRows;
    state.pipelineGridColumns = nextColumns;
    const nextSlots = normalizeGridSlots(nextRows, nextColumns, state.pipelineGridSlots);
    state.pipelineGridSlotOutputKeys = normalizeGridOutputKeys(nextRows, nextColumns, state.pipelineGridSlotOutputKeys);
    // When expanding from 1x1 into multiplex, do not implicitly carry over the raw stream into the
    // output cell. Multiplex grids should be empty unless the user explicitly places a pipeline.
    const expandingToMultiplex = nextMultiplex && !prevMultiplex;
    const prevWasJustRaw =
      Object.values(prevSlots).filter(Boolean).length === 1 && prevSlots[PIPELINE_OUTPUT_CELL_KEY] === RAW_PIPELINE_ID;
    state.pipelineGridSlots = expandingToMultiplex && prevWasJustRaw ? {} : nextSlots;

    // Only auto-ensure a pipeline in single-cell layouts. Multiplex grids are allowed to be empty.
    if (!nextMultiplex) {
      ensureAssignedPipelineId(state);
    }
    schedulePipelineLayoutApply();
  }

  const pipelineForCellAt = (row: number, column: number): string | null => pipelineForCellFn(state, row, column);
  const outputKeyForCellAt = (row: number, column: number): string | null => outputKeyForCellFn(state, row, column);
  const setPipelineForCellAt = (row: number, column: number, pipelineId: string | null): void =>
    setPipelineForCellFn(state, row, column, pipelineId, ensurePipelineOutputsLoaded);
  const setOutputKeyForCellAt = (row: number, column: number, outputKey: string | null): void =>
    setOutputKeyForCellFn(state, row, column, outputKey, schedulePipelineLayoutApply);

  function gridKey(row: number, column: number): string {
    return gridKeyFn(row, column);
  }

  function pipelineForCell(row: number, column: number): string | null {
    return pipelineForCellAt(row, column);
  }

  function setPipelineForCell(row: number, column: number, pipelineId: string | null): void {
    setPipelineForCellAt(row, column, pipelineId);
  }

  function outputKeyForCell(row: number, column: number): string | null {
    return outputKeyForCellAt(row, column);
  }

  function setOutputKeyForCell(row: number, column: number, outputKey: string | null): void {
    setOutputKeyForCellAt(row, column, outputKey);
  }

  function handlePipelineDragStart(pipelineId: string, from?: { row: number; column: number }): (event: DragEvent) => void {
    return (event: DragEvent) => {
      state.pipelineDragPayload = { pipelineId, from };
      try {
        event.dataTransfer?.setData('text/plain', JSON.stringify({ pipelineId, from }));
        event.dataTransfer?.setData('application/json', JSON.stringify({ pipelineId, from }));
      } catch {
        // ignore
      }
      if (event.dataTransfer) {
        event.dataTransfer.effectAllowed = 'move';
      }
    };
  }

  function readDragPayload(event: DragEvent): { pipelineId: string; from?: { row: number; column: number } } | null {
    if (state.pipelineDragPayload) return state.pipelineDragPayload;
    try {
      const raw = event.dataTransfer?.getData('application/json') || event.dataTransfer?.getData('text/plain');
      if (!raw) return null;
      const parsed = asRecord(JSON.parse(raw));
      const pipelineId = typeof parsed?.pipelineId === 'string' ? parsed.pipelineId.trim() : '';
      if (!pipelineId) return null;
      const fromRecord = asRecord(parsed?.from);
      const fromRow = typeof fromRecord?.row === 'number' ? fromRecord.row : null;
      const fromColumn = typeof fromRecord?.column === 'number' ? fromRecord.column : null;
      const from =
        fromRow != null && fromColumn != null && Number.isInteger(fromRow) && Number.isInteger(fromColumn)
          ? { row: fromRow, column: fromColumn }
          : undefined;
      return { pipelineId, from };
    } catch {
      return null;
    }
  }

  function allowDrop(event: DragEvent): void {
    event.preventDefault();
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = 'move';
    }
  }

  function dropOnCell(row: number, column: number): (event: DragEvent) => void {
    return (event: DragEvent) => {
      event.preventDefault();
      state.pipelineLayoutTouched = true;
      const payload = readDragPayload(event);
      state.pipelineDragPayload = null;
      if (!payload?.pipelineId) return;
      const pipelineId = payload.pipelineId;
      if (pipelineId === RAW_PIPELINE_ID) {
        setPipelineForCellAt(row, column, RAW_PIPELINE_ID);
        if (payload.from && (payload.from.row !== row || payload.from.column !== column)) {
          setPipelineForCellAt(payload.from.row, payload.from.column, null);
        }
        schedulePipelineLayoutApply();
        return;
      }
      setPipelineForCellAt(row, column, pipelineId);
      if (payload.from && (payload.from.row !== row || payload.from.column !== column)) {
        setPipelineForCellAt(payload.from.row, payload.from.column, null);
      }
      if (!state.assignedPipelineIds.includes(pipelineId)) {
        state.assignedPipelineIds = normalizeAssignedPipelineIds([pipelineId, ...state.assignedPipelineIds]);
      }
      schedulePipelineLayoutApply();
    };
  }

  function clearCell(row: number, column: number): void {
    state.pipelineLayoutTouched = true;
    setPipelineForCellAt(row, column, null);
    schedulePipelineLayoutApply();
  }

  function hydratePipelineUi(key: string): void {
    hydratePipelineUiState(state, deps.storagePrefix, key);
  }

  function persistPipelineUi(): void {
    persistPipelineUiState(state, deps.storagePrefix);
  }

  function pipelineUiStorageKey(key: string): string {
    return pipelineUiStorageKeyState(deps.storagePrefix, key);
  }

  function pipelineLayoutSignature(): string | null {
    return pipelineLayoutSignatureFn(state);
  }

  return {
    extractGraphOutputPorts,
    ensurePipelineRegistry,
    ensurePipelineGraphAndOutputs,
    ensurePipelineOutputsLoaded,
    outputSelectionForPipeline,
    isMultiplexLayout,
    gridHasUnappliedPipelines,
    isPipelineApplied,
    buildPipelineLayoutPayload,
    applyPipelineGraphForId,
    applyUnappliedPipelines,
    schedulePipelineLayoutApply,
    setOutputSelectionForPipeline,
    refreshPipelineGraphs,
    refreshPipelineOutputs,
    setLivePipelineOutput,
    setFrameSourceForPipelineInstance,
    pipelineLabel,
    listPipelineTemplatesForAssign,
    createPipelineFromTemplateAndAssign,
    openPipelineAssignModal,
    closePipelineAssignModal,
    savePipelineAssignModal,
    dropPipelineEverywhere,
    applyAssignedPipelineIds,
    openPipelineRemoveModal,
    closePipelineRemoveModal,
    confirmPipelineRemove,
    setPipelineGridDimensions,
    gridKey,
    pipelineForCell,
    setPipelineForCell,
    outputKeyForCell,
    setOutputKeyForCell,
    handlePipelineDragStart,
    readDragPayload,
    allowDrop,
    dropOnCell,
    clearCell,
    hydratePipelineUi,
    persistPipelineUi,
    pipelineUiStorageKey,
    pipelineLayoutSignature
  };
}
