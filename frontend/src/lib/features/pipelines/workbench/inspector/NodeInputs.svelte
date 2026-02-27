<script lang="ts">
  import {
    buildNodeValueFromInput,
    formatPipelineValue,
    getDataTypeVariants,
    isDataTypeSettable,
    resolveDataTypeKey
  } from '$lib/features/pipelines/valueFormatting';
  import {
    hexToRgb,
    isNumericTypeKey,
    isPixelTypeKey,
    parsePixelValue,
    pixelToHex
  } from '$lib/components/flow/pipeline-graph/editorUtils';
  import NodeInputsRangeCard from '$lib/features/pipelines/workbench/inspector/NodeInputsRangeCard.svelte';
  import NodeInputsEntryCard from '$lib/features/pipelines/workbench/inspector/NodeInputsEntryCard.svelte';
  import type { PipelineDetailContext } from '$lib';
  import type {
    PipelineDataType,
    PipelineGraphNode,
    PipelineNodeValue,
    PipelinePortMetadata,
    PipelineRegistryEntry,
    PipelineTypeDescriptor
  } from '$lib/types/pipeline';

  const {
    context,
    selectedNode,
    registryEntries = [],
    typePalette = {},
    constantsReadOnly = false,
    onSetConstantValue
  }: {
    context: PipelineDetailContext;
    selectedNode: PipelineGraphNode | null;
    registryEntries?: PipelineRegistryEntry[];
    typePalette?: Record<string, PipelineTypeDescriptor>;
    constantsReadOnly?: boolean;
    onSetConstantValue?: (payload: { nodeId: string; port: string; value: PipelineNodeValue | null }) => void;
  } = $props();

  type NodeParameterEntry = {
    port: string;
    key: string;
    dataType: PipelineDataType | string | undefined;
    baseValue: PipelineNodeValue | undefined;
    overrideValue: PipelineNodeValue | undefined;
    variants: string[];
    settable: boolean;
    metadata?: PipelinePortMetadata;
  };

  type RangeParameterItem = {
    kind: 'range';
    base: string;
    minEntry: NodeParameterEntry;
    maxEntry: NodeParameterEntry;
  };

  type NodeParameterRenderItem =
    | RangeParameterItem
    | {
        kind: 'entry';
        entry: NodeParameterEntry;
      };

  const parseNumericValue = (raw: string): number | null => {
    if (raw == null) return null;
    const parsed = Number(raw);
    return Number.isFinite(parsed) ? parsed : null;
  };

  const rangeBaseForKey = (key: string): string | null => {
    const match = key.match(/^(.*)_(min|max)$/i);
    return match?.[1] ?? null;
  };

  const isRangeControl = (entry: NodeParameterEntry | undefined): boolean => {
    if (!entry) return false;
    const control = entry.metadata?.uiControl?.trim().toLowerCase();
    if (control !== 'range') return false;
    return Boolean(rangeBaseForKey(entry.key));
  };

  const precisionFromStep = (step?: number): number => {
    if (!Number.isFinite(step)) return 3;
    const safe = Number(step);
    if (!Number.isFinite(safe) || safe <= 0) return 3;
    if (Number.isInteger(safe)) return 0;
    const raw = safe.toString();
    if (raw.includes('e-')) {
      const exp = Number(raw.split('e-')[1]);
      return Number.isFinite(exp) ? exp : 3;
    }
    const idx = raw.indexOf('.');
    return idx >= 0 ? raw.length - idx - 1 : 0;
  };

  const formatRangeValue = (value: number, step?: number): string => {
    const precision = precisionFromStep(step);
    const rounded = precision > 0 ? Number(value.toFixed(precision)) : Math.round(value);
    return String(rounded);
  };

  const normalizeRangeBase = (base: string): string => {
    const key = base.trim().toLowerCase();
    if (key === 'hue') return 'h';
    if (key === 'sat' || key === 'saturation') return 's';
    if (key === 'val' || key === 'value') return 'v';
    if (key === 'red') return 'r';
    if (key === 'green') return 'g';
    if (key === 'blue') return 'b';
    return key;
  };

  const rangeLabelForBase = (base: string): string => {
    const key = normalizeRangeBase(base);
    if (key === 'h') return 'Hue Range';
    if (key === 's') return 'Saturation Range';
    if (key === 'v') return 'Value Range';
    if (key === 'r') return 'Red Range';
    if (key === 'g') return 'Green Range';
    if (key === 'b') return 'Blue Range';
    return `${base.toUpperCase()} Range`;
  };

  const hueGradientStops = [0, 60, 120, 180, 240, 300, 360]
    .map((hue) => `hsl(${hue}, 100%, 50%)`)
    .join(', ');
  const hueGradient = `linear-gradient(to right, ${hueGradientStops})`;

  const resolvePortMetadata = (
    metadata: Record<string, PipelinePortMetadata> | null | undefined,
    port: string,
    key: string
  ): PipelinePortMetadata | undefined => {
    if (!metadata) return undefined;
    return metadata[port] ?? metadata[key] ?? metadata[port.toLowerCase()] ?? metadata[key.toLowerCase()];
  };

  const nodeParameterEntries = $derived.by(() => {
    if (!selectedNode || !context.pipeline || !context.graphSelectionNodeId) {
      return [] as NodeParameterEntry[];
    }
    const inputs = selectedNode.inputs ?? {};
    const baseValues = selectedNode.info.values ?? {};
    const overrideValues = context.pipeline.graph.nodeValueOverrides?.[context.graphSelectionNodeId] ?? {};
    const entries = new Map<string, NodeParameterEntry>();
    const registryEntry = registryEntries.find((candidate) => candidate.id === selectedNode.backendId);

    const addEntry = (portLabel: string, key: string, descriptor?: PipelineDataType) => {
      const baseValue = baseValues[key];
      const overrideValue = overrideValues[key];
      if (!descriptor && !baseValue && !overrideValue) {
        return;
      }
      const registryMetadata = registryEntry?.metadata?.inputPorts ?? null;
      const portMetadata =
        resolvePortMetadata(selectedNode.metadata?.inputPorts ?? null, portLabel, key) ??
        resolvePortMetadata(registryMetadata, portLabel, key);
      const registryInputType =
        registryEntry?.inputs?.[portLabel] ??
        registryEntry?.inputs?.[key] ??
        registryEntry?.inputs?.[portLabel.toLowerCase()] ??
        registryEntry?.inputs?.[key.toLowerCase()];
      const dataType = descriptor ?? registryInputType ?? baseValue?.dataType ?? overrideValue?.dataType;
      const variants = getDataTypeVariants(dataType);
      const settable = isDataTypeSettable(dataType);
      entries.set(key, {
        port: portLabel,
        key,
        dataType,
        baseValue,
        overrideValue,
        variants,
        settable,
        metadata: portMetadata
      });
    };

    Object.entries(inputs).forEach(([port, descriptor]) => {
      addEntry(port, port.toLowerCase(), descriptor);
    });

    Object.keys(baseValues).forEach((key) => {
      if (!entries.has(key)) {
        addEntry(key, key);
      }
    });
    Object.keys(overrideValues).forEach((key) => {
      if (!entries.has(key)) {
        addEntry(key, key);
      }
    });
    return Array.from(entries.values()).sort((a, b) => a.port.localeCompare(b.port));
  });

  let nodeParameterDrafts = $state<Record<string, string>>({});
  let nodeParameterErrors = $state<Record<string, string | null>>({});
  let nodeParameterSearch = $state('');
  const normalizedParameterSearch = $derived.by(() => nodeParameterSearch.trim().toLowerCase());

  $effect(() => {
    if (!selectedNode || nodeParameterEntries.length === 0) {
      nodeParameterDrafts = {};
      nodeParameterErrors = {};
      return;
    }
    const drafts: Record<string, string> = {};
    nodeParameterEntries.forEach((entry) => {
      const initialValue = entry.overrideValue ?? entry.baseValue;
      const typeKey = resolveDataTypeKey(entry.dataType ?? entry.baseValue?.dataType ?? entry.overrideValue?.dataType) ?? null;
      const pixelValue = typeKey && isPixelTypeKey(typeKey) ? parsePixelValue(initialValue ?? null) : null;
      if (pixelValue) {
        drafts[entry.key] = pixelToHex(pixelValue);
      } else if (initialValue && typeof initialValue.value === 'string') {
        drafts[entry.key] = initialValue.value;
      } else {
        drafts[entry.key] = formatPipelineValue(initialValue) ?? '';
      }
    });
    nodeParameterDrafts = drafts;
    nodeParameterErrors = {};
  });

  const filteredNodeParameterEntries = $derived.by(() => {
    if (!normalizedParameterSearch) return nodeParameterEntries;
    return nodeParameterEntries.filter((entry) => {
      const haystack = `${entry.port} ${entry.key}`.toLowerCase();
      return haystack.includes(normalizedParameterSearch);
    });
  });

  const nodeParameterEntryMap = $derived.by(() => {
    const map: Record<string, NodeParameterEntry> = {};
    nodeParameterEntries.forEach((entry) => {
      map[entry.key] = entry;
    });
    return map;
  });

  const nodeParameterRenderItems = $derived.by<NodeParameterRenderItem[]>(() => {
    const items: NodeParameterRenderItem[] = [];
    const used = new Set<string>();
    const allEntries = nodeParameterEntryMap;

    for (const entry of filteredNodeParameterEntries) {
      if (used.has(entry.key)) continue;
      if (isRangeControl(entry)) {
        const base = rangeBaseForKey(entry.key);
        if (base) {
          const minEntry = allEntries[`${base}_min`];
          const maxEntry = allEntries[`${base}_max`];
          if (minEntry && maxEntry && minEntry.settable && maxEntry.settable && isRangeControl(minEntry) && isRangeControl(maxEntry)) {
            items.push({ kind: 'range', base, minEntry, maxEntry });
            used.add(minEntry.key);
            used.add(maxEntry.key);
            continue;
          }
        }
      }
      items.push({ kind: 'entry', entry });
      used.add(entry.key);
    }
    return items;
  });

  const hueReference = $derived.by(() => {
    const hMin = parseNumericValue(nodeParameterDrafts['h_min'] ?? '') ?? null;
    const hMax = parseNumericValue(nodeParameterDrafts['h_max'] ?? '') ?? null;
    if (hMin == null && hMax == null) return 200;
    const left = hMin ?? hMax ?? 0;
    const right = hMax ?? hMin ?? 0;
    let mid = (left + right) / 2;
    if (!Number.isFinite(mid)) return 200;
    mid = ((mid % 360) + 360) % 360;
    return mid;
  });

  const rangeGradientForBase = (base: string, hue: number): string => {
    const key = normalizeRangeBase(base);
    if (key === 'h') return hueGradient;
    if (key === 's') {
      return `linear-gradient(to right, hsl(${hue}, 0%, 50%), hsl(${hue}, 100%, 50%))`;
    }
    if (key === 'v') {
      return `linear-gradient(to right, #000, hsl(${hue}, 100%, 50%))`;
    }
    if (key === 'r') return 'linear-gradient(to right, rgb(0, 0, 0), rgb(255, 64, 64))';
    if (key === 'g') return 'linear-gradient(to right, rgb(0, 0, 0), rgb(64, 255, 64))';
    if (key === 'b') return 'linear-gradient(to right, rgb(0, 0, 0), rgb(64, 128, 255))';
    return 'linear-gradient(to right, #111, #ddd)';
  };

  const colorRangeBases = new Set(['h', 's', 'v', 'r', 'g', 'b']);

  const rgbToHsv = (rgb: { r: number; g: number; b: number }) => {
    const r = rgb.r / 255;
    const g = rgb.g / 255;
    const b = rgb.b / 255;
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const delta = max - min;
    let h = 0;
    if (delta !== 0) {
      if (max === r) {
        h = ((g - b) / delta) % 6;
      } else if (max === g) {
        h = (b - r) / delta + 2;
      } else {
        h = (r - g) / delta + 4;
      }
      h *= 60;
      if (h < 0) h += 360;
    }
    const s = max === 0 ? 0 : delta / max;
    const v = max;
    return { h, s, v };
  };

  const normalizedChannelForBase = (base: string, rgb: { r: number; g: number; b: number }) => {
    const key = normalizeRangeBase(base);
    if (key === 'r') return rgb.r / 255;
    if (key === 'g') return rgb.g / 255;
    if (key === 'b') return rgb.b / 255;
    const hsv = rgbToHsv(rgb);
    if (key === 'h') return hsv.h / 360;
    if (key === 's') return hsv.s;
    if (key === 'v') return hsv.v;
    return null;
  };

  const mapNormalizedToRange = (normalized: number, min: number, max: number): number => {
    const clamped = Math.min(1, Math.max(0, normalized));
    return min + clamped * (max - min);
  };

  const applyDropperToRange = (
    base: string,
    hex: string,
    sliderMin: number,
    sliderMax: number,
    sliderStep: number | undefined,
    minEntry: NodeParameterEntry,
    maxEntry: NodeParameterEntry
  ) => {
    const rgb = hexToRgb(hex);
    if (!rgb) return;
    const normalized = normalizedChannelForBase(base, rgb);
    if (normalized == null || !Number.isFinite(normalized)) return;
    const value = mapNormalizedToRange(normalized, sliderMin, sliderMax);
    updateRangeDrafts(minEntry.key, maxEntry.key, value, value, sliderStep);
  };

  function updateNodeParameterDraft(key: string, value: string) {
    nodeParameterDrafts = { ...nodeParameterDrafts, [key]: value };
  }

  function updateRangeDrafts(minKey: string, maxKey: string, minValue: number, maxValue: number, step?: number) {
    const nextMin = formatRangeValue(minValue, step);
    const nextMax = formatRangeValue(maxValue, step);
    nodeParameterDrafts = { ...nodeParameterDrafts, [minKey]: nextMin, [maxKey]: nextMax };
  }

  function applyRangeParameters(minEntry: NodeParameterEntry, maxEntry: NodeParameterEntry) {
    applyNodeParameter(minEntry);
    applyNodeParameter(maxEntry);
  }

  function clearRangeParameters(minEntry: NodeParameterEntry, maxEntry: NodeParameterEntry) {
    clearNodeParameter(minEntry);
    clearNodeParameter(maxEntry);
  }

  function applyNodeParameter(entry: NodeParameterEntry) {
    if (!selectedNode || !context.graphSelectionNodeId || constantsReadOnly) return;
    if (!entry.settable) {
      nodeParameterErrors = { ...nodeParameterErrors, [entry.key]: 'Port is not settable' };
      return;
    }
    const rawValue = nodeParameterDrafts[entry.key] ?? '';
    const typeKey = resolveDataTypeKey(entry.dataType ?? entry.baseValue?.dataType ?? entry.overrideValue?.dataType) ?? 'string';
    if (isNumericTypeKey(typeKey)) {
      const numeric = parseNumericValue(rawValue);
      const absoluteMin = entry.metadata?.min;
      const absoluteMax = entry.metadata?.max;
      if (numeric == null) {
        nodeParameterErrors = { ...nodeParameterErrors, [entry.key]: 'Enter a number' };
        return;
      }
      if (absoluteMin !== undefined && numeric < absoluteMin) {
        nodeParameterErrors = { ...nodeParameterErrors, [entry.key]: `Minimum allowed is ${absoluteMin}` };
        return;
      }
      if (absoluteMax !== undefined && numeric > absoluteMax) {
        nodeParameterErrors = { ...nodeParameterErrors, [entry.key]: `Maximum allowed is ${absoluteMax}` };
        return;
      }
    }
    if (isPixelTypeKey(typeKey)) {
      const rgb = hexToRgb(rawValue);
      if (!rgb) {
        nodeParameterErrors = { ...nodeParameterErrors, [entry.key]: 'Invalid color' };
        return;
      }
      const dataType = entry.dataType ?? entry.baseValue?.dataType ?? { kind: 'pixel' };
      const value: PipelineNodeValue = {
        dataType,
        value: { r: rgb.r, g: rgb.g, b: rgb.b, a: 255 }
      };
      onSetConstantValue?.({
        nodeId: context.graphSelectionNodeId,
        port: entry.key,
        value
      });
      nodeParameterErrors = { ...nodeParameterErrors, [entry.key]: null };
      return;
    }
    const result = buildNodeValueFromInput(rawValue, typeKey);
    if (!result.success) {
      nodeParameterErrors = { ...nodeParameterErrors, [entry.key]: result.error ?? 'Invalid value' };
      return;
    }
    onSetConstantValue?.({
      nodeId: context.graphSelectionNodeId,
      port: entry.key,
      value: result.value
    });
    nodeParameterErrors = { ...nodeParameterErrors, [entry.key]: null };
  }

  function clearNodeParameter(entry: NodeParameterEntry) {
    if (!context.graphSelectionNodeId || constantsReadOnly || !entry.settable) return;
    onSetConstantValue?.({
      nodeId: context.graphSelectionNodeId,
      port: entry.key,
      value: null
    });
    nodeParameterErrors = { ...nodeParameterErrors, [entry.key]: null };
  }
</script>

<section class="space-y-3">
  <h3 class="text-[0.7rem] uppercase tracking-[0.3em] text-surface-500">Constants & Overrides</h3>
  {#if constantsReadOnly}
    <p class="text-xs text-surface-400">
      Ports on linked pipelines are read-only. Open the referenced pipeline to edit its inputs.
    </p>
  {/if}
  {#if nodeParameterEntries.length === 0}
    <p class="text-xs text-surface-500">This node does not expose editable constants.</p>
  {:else}
    <div class="space-y-3 text-xs">
      <div class="flex items-center gap-2 rounded border border-surface-800/70 bg-surface-900/40 px-2 py-1">
        <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Search</span>
        <input
          class="flex-1 border-0 bg-transparent text-xs text-surface-100 placeholder:text-surface-500 focus:outline-none"
          placeholder="Filter constants"
          value={nodeParameterSearch}
          oninput={(event) => (nodeParameterSearch = (event.currentTarget as HTMLInputElement).value)}
        />
      </div>
      {#if nodeParameterRenderItems.length === 0}
        <p class="text-xs text-surface-500">No constants match this search.</p>
      {:else}
        {#each nodeParameterRenderItems as item (item.kind === 'range' ? `range:${item.base}` : item.entry.key)}
          {#if item.kind === 'range'}
            {@const minEntry = item.minEntry}
            {@const maxEntry = item.maxEntry}
            {@const base = item.base}
            {@const label = rangeLabelForBase(base)}
            {@const dataType = minEntry.dataType ?? maxEntry.dataType ?? minEntry.baseValue?.dataType ?? maxEntry.baseValue?.dataType}
            {@const resolvedTypeKey = resolveDataTypeKey(dataType) ?? 'string'}
            {@const sliderMin = minEntry.metadata?.uiMin ?? minEntry.metadata?.min ?? 0}
            {@const sliderMax = minEntry.metadata?.uiMax ?? minEntry.metadata?.max ?? 1}
            {@const sliderStep = minEntry.metadata?.uiStep ?? minEntry.metadata?.step ?? (['int', 'uint', 'sint'].includes(resolvedTypeKey) ? 1 : undefined)}
            {@const minDraft = parseNumericValue(nodeParameterDrafts[minEntry.key] ?? '') ?? sliderMin}
            {@const maxDraft = parseNumericValue(nodeParameterDrafts[maxEntry.key] ?? '') ?? sliderMax}
            {@const rangeGradient = rangeGradientForBase(base, hueReference)}
            {@const hasOverride = Boolean(minEntry.overrideValue || maxEntry.overrideValue)}
            {@const rangeError = nodeParameterErrors[minEntry.key] ?? nodeParameterErrors[maxEntry.key] ?? null}
            <NodeInputsRangeCard
              {label}
              {dataType}
              {typePalette}
              {minEntry}
              {maxEntry}
              {sliderMin}
              {sliderMax}
              {sliderStep}
              {minDraft}
              {maxDraft}
              {rangeGradient}
              {hasOverride}
              {rangeError}
              constantsReadOnly={constantsReadOnly}
              showDropper={colorRangeBases.has(normalizeRangeBase(base))}
              onDropperPick={(hex) => applyDropperToRange(base, hex, sliderMin, sliderMax, sliderStep, minEntry, maxEntry)}
              onRangeChange={(detail) => updateRangeDrafts(minEntry.key, maxEntry.key, detail.min, detail.max, sliderStep)}
              onMinInput={(value) => updateNodeParameterDraft(minEntry.key, value)}
              onMaxInput={(value) => updateNodeParameterDraft(maxEntry.key, value)}
              onApply={() => applyRangeParameters(minEntry, maxEntry)}
              onClear={() => clearRangeParameters(minEntry, maxEntry)}
            />
          {:else}
            {@const entry = item.entry}
            <NodeInputsEntryCard
              {entry}
              {typePalette}
              draftValue={nodeParameterDrafts[entry.key] ?? ''}
              error={nodeParameterErrors[entry.key] ?? null}
              constantsReadOnly={constantsReadOnly}
              onDraftChange={(value) => updateNodeParameterDraft(entry.key, value)}
              onApply={() => applyNodeParameter(entry)}
              onClear={() => clearNodeParameter(entry)}
            />
          {/if}
        {/each}
      {/if}
    </div>
  {/if}
</section>
