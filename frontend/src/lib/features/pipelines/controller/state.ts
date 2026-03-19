import { derived, writable, type Readable } from 'svelte/store';
import type {
  PipelineGraphNode,
  PipelineGraphPlan,
  PipelineOverviewPipeline,
  PipelineRegistryEntry
} from '$lib/types/pipeline';
import type { PipelineOutputEntry } from '$lib/components/pipelines/types';
import type { PipelineBreadcrumb } from './types';
import type { PipelineMetricsState } from './pipelineMetrics';
import { resolvePlanAtPath } from '../nesting';
import { formatStatus, statusBadgeClass } from './uiAdapters';
import { createPipelineListWorker, createPipelineRegistryWorker } from '$lib/workers/factories';

type GraphSelection = { nodeId: string | null; nodes: string[]; edge: { id: string } | null };

export function createPipelineListState(params: {
  pipelines: Readable<PipelineOverviewPipeline[]>;
  pipelineSearch: Readable<string>;
  selectedPipelineId: Readable<string | null>;
  editingPath: Readable<string[]>;
}) {
  type PipelineListItem = {
    id: string;
    name: string;
    alias: string;
    appearance: PipelineOverviewPipeline['appearance'] | null;
    status: PipelineOverviewPipeline['status'];
    statusLabel: string;
    statusClass: string;
    attachments: number;
    issueCount: number;
    revision: string | null;
  };

  const summary = writable({ total: 0, live: 0, degraded: 0, drafts: 0 });
  const pipelineListItems = writable<PipelineListItem[]>([]);

  const filteredPipelines = derived([params.pipelines, params.pipelineSearch], ([$pipelines, $search]) => {
    const term = $search.trim().toLowerCase();
    if (!term) return $pipelines.slice();
    return $pipelines.filter((pipeline) => `${pipeline.name} ${pipeline.alias}`.toLowerCase().includes(term));
  });

  let worker: Worker | null = null;
  let requestId = 0;
  let lastHandled = 0;

  const updateFallback = (pipelines: PipelineOverviewPipeline[], search: string) => {
    const live = pipelines.filter((pipeline) => pipeline.status === 'live').length;
    const degraded = pipelines.filter((pipeline) => pipeline.status === 'degraded').length;
    const drafts = pipelines.filter((pipeline) => pipeline.status === 'draft').length;
    summary.set({ total: pipelines.length, live, degraded, drafts });
    const term = search.trim().toLowerCase();
    const filtered = term.length
      ? pipelines.filter((pipeline) => `${pipeline.name} ${pipeline.alias}`.toLowerCase().includes(term))
      : pipelines.slice();
    pipelineListItems.set(
      filtered.map((pipeline) => ({
        id: pipeline.id,
        name: pipeline.name,
        alias: pipeline.alias,
        appearance: pipeline.appearance ?? null,
        status: pipeline.status,
        statusLabel: formatStatus(pipeline.status),
        statusClass: statusBadgeClass(pipeline.status),
        attachments: pipeline.attachments.length,
        issueCount: pipeline.issueCount ?? 0,
        revision: pipeline.revision ?? null
      }))
    );
  };

  if (typeof Worker !== 'undefined') {
    worker = createPipelineListWorker();
    worker.onmessage = (event) => {
      const data = event.data as {
        requestId: number;
        summary: { total: number; live: number; degraded: number; drafts: number };
        listItems: PipelineListItem[];
      };
      if (data.requestId < lastHandled) return;
      lastHandled = data.requestId;
      summary.set(data.summary ?? { total: 0, live: 0, degraded: 0, drafts: 0 });
      pipelineListItems.set(Array.isArray(data.listItems) ? data.listItems : []);
    };
  }

  const unsubscribe = derived([params.pipelines, params.pipelineSearch], ([$pipelines, $search]) => ({
    pipelines: $pipelines,
    search: $search ?? ''
  })).subscribe((payload) => {
    if (!worker) {
      updateFallback(payload.pipelines, payload.search);
      return;
    }
    const nextRequestId = ++requestId;
    const pipelineViews = payload.pipelines.map((pipeline) => ({
      id: pipeline.id,
      name: pipeline.name,
      alias: pipeline.alias,
      appearance: pipeline.appearance ?? null,
      status: pipeline.status,
      attachments: pipeline.attachments.length,
      issueCount: pipeline.issueCount ?? 0,
      revision: pipeline.revision ?? null
    }));
    worker.postMessage({ requestId: nextRequestId, pipelines: pipelineViews, search: payload.search });
  });

  const selectedPipeline = derived([params.pipelines, params.selectedPipelineId], ([$pipelines, $selectedId]) =>
    $pipelines.find((pipeline) => pipeline.id === $selectedId) ?? null
  );

  const editingPlan = derived([selectedPipeline, params.editingPath], ([$pipeline, $path]) => {
    if (!$pipeline) return null;
    return resolvePlanAtPath($pipeline.graph, $path) ?? $pipeline.graph;
  });

  const editingBreadcrumbs = derived([selectedPipeline, params.editingPath], ([$pipeline, $path]) => {
    if (!$pipeline) return [] as PipelineBreadcrumb[];
    const breadcrumbs: PipelineBreadcrumb[] = [];
    const nodeWarnings = $pipeline.diagnostics?.warnings ?? [];
    let current: PipelineGraphPlan | null = $pipeline.graph;
    for (const nodeId of $path) {
      if (!current) break;
      const node: PipelineGraphNode | undefined = current.nodes?.[nodeId];
      if (!node?.embedded) break;
      const name = node.metadata?.name?.trim() || node.info?.id || node.backendId || nodeId;
      const external = node.external;
      const alias = external?.alias?.trim() || null;
      const targetId = external?.pipelineId?.trim() || null;
      const warnings = nodeWarnings.filter((warning) => warning.nodeId === nodeId).map((warning) => warning.message.toLowerCase());
      const mismatch = warnings.some((msg) => msg.includes('mismatch'));
      const unresolved = warnings.some((msg) => msg.includes('missing') || msg.includes('resolve') || msg.includes('cycle'));
      const status: PipelineBreadcrumb['status'] =
        node.embedded && !external
          ? 'embedded'
          : external
            ? mismatch
              ? 'mismatch'
              : unresolved
                ? 'unresolved'
                : 'linked'
            : undefined;
      breadcrumbs.push({ id: nodeId, name, status, targetId: alias ?? targetId });
      current = node.embedded;
    }
    return breadcrumbs;
  });

  return {
    summary,
    filteredPipelines,
    pipelineListItems,
    selectedPipeline,
    editingPlan,
    editingBreadcrumbs,
    dispose: () => {
      unsubscribe();
      if (worker) {
        worker.terminate();
        worker = null;
      }
    }
  };
}

export function createPipelineDetailContext(params: {
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
  dirtyState: Readable<Record<string, boolean>>;
  saveState: Readable<Record<string, 'idle' | 'saving' | 'error'>>;
  validationMessages: Readable<Record<string, { ok: boolean; warnings: string[] }>>;
  pipelineInputEntries: Readable<unknown[]>;
  pipelineOutputEntries: Readable<PipelineOutputEntry[]>;
  graphSelection: Readable<GraphSelection>;
  detachBusyMap: Readable<Record<string, boolean>>;
  pipelineMetricsState: Readable<Record<string, PipelineMetricsState>>;
}) {
  return derived(
    [
      params.selectedPipeline,
      params.dirtyState,
      params.saveState,
      params.validationMessages,
      params.pipelineInputEntries,
      params.pipelineOutputEntries,
      params.graphSelection,
      params.detachBusyMap,
      params.pipelineMetricsState
    ],
    ([$pipeline, $dirty, $save, $validation, $inputs, $outputs, $selection, $detach, $metrics]) => {
      const metricsState = $pipeline
        ? $metrics[$pipeline.id] ?? { status: 'idle', metrics: null, error: null, updatedAt: null }
        : { status: 'idle', metrics: null, error: null, updatedAt: null };
      return {
        pipeline: $pipeline
          ? {
              id: $pipeline.id,
              name: $pipeline.name,
              alias: $pipeline.alias,
              status: $pipeline.status,
              revision: $pipeline.revision ?? null,
              planHash: $pipeline.planHash ?? null,
              updatedAt: $pipeline.updatedAt ?? null,
              graph: $pipeline.graph,
              attachments: $pipeline.attachments,
              diagnostics: $pipeline.diagnostics ?? null
            }
          : null,
        dirty: $pipeline ? Boolean($dirty[$pipeline.id]) : false,
        savingState: $pipeline ? $save[$pipeline.id] ?? 'idle' : 'idle',
        validation: $pipeline ? $validation[$pipeline.id] ?? null : null,
        pipelineInputs: $inputs,
        pipelineOutputs: $outputs,
        graphSelectionNodeId: $selection.nodeId,
        graphSelectionEdgeId: $selection.edge?.id ?? null,
        detachBusyMap: $detach,
        metrics: metricsState.metrics,
        metricsStatus: metricsState.status,
        metricsError: metricsState.error,
        metricsUpdatedAt: metricsState.updatedAt
      };
    }
  );
}

export function createRegistryFilterState(params: {
  registry: Readable<PipelineRegistryEntry[]>;
  registrySearch: Readable<string>;
  registryTag: Readable<string | null>;
  registryCategory: Readable<string | null>;
  registryProvider: Readable<string | null>;
  activeRegistryGroup: Readable<string>;
  registrySort: Readable<'name-asc' | 'name-desc' | 'id-asc'>;
}) {
  const availableTags = writable<string[]>([]);
  const availableCategories = writable<string[]>([]);
  const availableProviders = writable<string[]>([]);
  const hasActiveRegistryFilters = writable(false);
  const registryEntriesFiltered = writable<PipelineRegistryEntry[]>([]);
  const visibleRegistryEntries = writable<PipelineRegistryEntry[]>([]);
  const registryGroups = writable<{ key: string; label: string; entries: PipelineRegistryEntry[] }[]>([]);

  let worker: Worker | null = null;
  let requestId = 0;
  let lastHandled = 0;
  let lastPayload: {
    entries: PipelineRegistryEntry[];
    search: string;
    tag: string | null;
    category: string | null;
    provider: string | null;
    activeGroup: string;
    sort: 'name-asc' | 'name-desc' | 'id-asc';
  } = {
    entries: [],
    search: '',
    tag: null,
    category: null,
    provider: null,
    activeGroup: 'all',
    sort: 'name-asc'
  };

  const updateFallback = (payload: {
    entries: PipelineRegistryEntry[];
    search: string;
    tag: string | null;
    category: string | null;
    provider: string | null;
    activeGroup: string;
    sort: 'name-asc' | 'name-desc' | 'id-asc';
  }) => {
    const normalizeString = (value: string | null | undefined) => (value ?? '').trim().toLowerCase();
    const tags = new Set<string>();
    const categories = new Set<string>();
    const providers = new Set<string>();
    const displayEntries = payload.entries.filter((entry) => {
      const provider = normalizeString(entry.metadata.provider);
      if (provider && /^(io|host)(?:$|[-_:._ ])/.test(provider)) return false;
      const id = normalizeString(entry.id);
      if (/^(io|host)[-_:._]/.test(id)) return false;
      return true;
    });
    displayEntries.forEach((entry) => {
      entry.metadata.tags?.forEach((tag) => tags.add(tag));
      entry.metadata.categories?.forEach((group) => group.forEach((category) => categories.add(category)));
      if (entry.metadata.provider) providers.add(entry.metadata.provider);
    });
    const tagList = Array.from(tags).sort((a, b) => a.localeCompare(b));
    const categoryList = Array.from(categories).sort((a, b) => a.localeCompare(b));
    const providerList = Array.from(providers).sort((a, b) => a.localeCompare(b));
    const normalizedSearch = normalizeString(payload.search);
    const normalizedCategory = normalizeString(payload.category);
    const normalizedProvider = normalizeString(payload.provider);
    const filtered = displayEntries.filter((entry) => {
      if (payload.tag && !(entry.metadata.tags ?? []).includes(payload.tag)) return false;
      if (normalizedCategory) {
        const match = (entry.metadata.categories ?? []).some((path) =>
          path.some((segment) => normalizeString(segment) === normalizedCategory)
        );
        if (!match) return false;
      }
      if (normalizedProvider) {
        const entryProvider = normalizeString(entry.metadata.provider);
        if (entryProvider !== normalizedProvider) return false;
      }
      if (normalizedSearch) {
        const ports = [...Object.keys(entry.inputs ?? {}), ...Object.keys(entry.outputs ?? {})];
        const haystack = [
          entry.id,
          entry.metadata.name,
          entry.metadata.provider ?? '',
          ...(entry.metadata.tags ?? []),
          ...ports
        ]
          .join(' ')
          .toLowerCase();
        if (!haystack.includes(normalizedSearch)) return false;
      }
      return true;
    });
    const groups = new Map<string, { key: string; label: string; entries: PipelineRegistryEntry[] }>();
    filtered.forEach((entry) => {
      const primary = entry.metadata.categories?.find((path) => path.length > 0)?.[0] ?? null;
      const key = normalizeString(primary ?? 'uncategorized');
      const label = primary ?? 'Uncategorized';
      const bucket = groups.get(key);
      if (bucket) {
        bucket.entries.push(entry);
      } else {
        groups.set(key, { key, label, entries: [entry] });
      }
    });
    const grouped = payload.activeGroup && payload.activeGroup !== 'all'
      ? filtered.filter((entry) => {
          const primary = entry.metadata.categories?.find((path) => path.length > 0)?.[0] ?? null;
          const key = normalizeString(primary ?? 'uncategorized');
          return key === normalizeString(payload.activeGroup);
        })
      : filtered;
    const visible = grouped.slice();
    switch (payload.sort) {
      case 'name-desc':
        visible.sort((a, b) => b.metadata.name.localeCompare(a.metadata.name));
        break;
      case 'id-asc':
        visible.sort((a, b) => a.id.localeCompare(b.id));
        break;
      default:
        visible.sort((a, b) => a.metadata.name.localeCompare(b.metadata.name));
        break;
    }
    availableTags.set(tagList);
    availableCategories.set(categoryList);
    availableProviders.set(providerList);
    hasActiveRegistryFilters.set(
      Boolean(payload.tag || payload.category || payload.provider || (payload.activeGroup && payload.activeGroup !== 'all'))
    );
    registryEntriesFiltered.set(filtered);
    registryGroups.set(
      Array.from(groups.values()).map((group) => ({
        ...group,
        entries: group.entries.slice().sort((a, b) => a.metadata.name.localeCompare(b.metadata.name))
      }))
    );
    visibleRegistryEntries.set(visible);
  };

  if (typeof Worker !== 'undefined') {
    worker = createPipelineRegistryWorker();
    worker.onmessage = (event) => {
      const data = event.data as {
        requestId: number;
        availableTags: string[];
        availableCategories: string[];
        availableProviders: string[];
        hasActiveFilters: boolean;
        entriesFiltered: PipelineRegistryEntry[];
        visibleEntries: PipelineRegistryEntry[];
        groups: { key: string; label: string; entries: PipelineRegistryEntry[] }[];
        error?: string;
      };
      if (data?.error) {
        updateFallback(lastPayload);
        return;
      }
      if (data.requestId < lastHandled) return;
      lastHandled = data.requestId;
      availableTags.set(data.availableTags ?? []);
      availableCategories.set(data.availableCategories ?? []);
      availableProviders.set(data.availableProviders ?? []);
      hasActiveRegistryFilters.set(Boolean(data.hasActiveFilters));
      registryEntriesFiltered.set(Array.isArray(data.entriesFiltered) ? data.entriesFiltered : []);
      visibleRegistryEntries.set(Array.isArray(data.visibleEntries) ? data.visibleEntries : []);
      registryGroups.set(Array.isArray(data.groups) ? data.groups : []);
    };
    worker.onerror = () => {
      if (worker) {
        worker.terminate();
        worker = null;
      }
      updateFallback(lastPayload);
    };
  }

  const unsubscribe = derived(
    [
      params.registry,
      params.registrySearch,
      params.registryTag,
      params.registryCategory,
      params.registryProvider,
      params.activeRegistryGroup,
      params.registrySort
    ],
    ([$registry, $search, $tag, $category, $provider, $group, $sort]) => ({
      entries: $registry,
      search: $search ?? '',
      tag: $tag ?? null,
      category: $category ?? null,
      provider: $provider ?? null,
      activeGroup: $group ?? 'all',
      sort: $sort ?? 'name-asc'
    })
  ).subscribe((payload) => {
    lastPayload = payload;
    if (!worker) {
      updateFallback(payload);
      return;
    }
    const nextRequestId = ++requestId;
    worker.postMessage({ requestId: nextRequestId, ...payload });
  });

  return {
    availableTags,
    availableCategories,
    availableProviders,
    hasActiveRegistryFilters,
    registryEntriesFiltered,
    visibleRegistryEntries,
    registryGroups,
    dispose: () => {
      unsubscribe();
      if (worker) {
        worker.terminate();
        worker = null;
      }
    }
  };
}
