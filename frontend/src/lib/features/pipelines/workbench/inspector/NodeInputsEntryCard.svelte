<script lang="ts">
  import ColorDropperButton from '$lib/components/controls/ColorDropperButton.svelte';
  import { describePortType } from '$lib/features/pipelines/inspector/inspectorTypeUtils';
  import { formatPipelineValue, parseEnumVariant, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
  import { isNumericTypeKey, isPixelTypeKey } from '$lib/components/flow/pipeline-graph/editorUtils';
  import type { PipelineDataType, PipelineNodeValue, PipelinePortMetadata, PipelineTypeDescriptor } from '$lib/types/pipeline';
  import { SvelteSet } from 'svelte/reactivity';

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

  const props = $props<{
    entry: NodeParameterEntry;
    typePalette: Record<string, PipelineTypeDescriptor>;
    draftValue: string;
    error: string | null;
    constantsReadOnly: boolean;
    onDraftChange: (value: string) => void;
    onApply: () => void;
    onClear: () => void;
  }>();

  const entry = $derived.by(() => props.entry);
  const typePalette = $derived.by(() => props.typePalette);
  const draftValue = $derived.by(() => props.draftValue);
  const error = $derived.by(() => props.error);
  const constantsReadOnly = $derived.by(() => props.constantsReadOnly);
  const onDraftChange = (value: string): void => props.onDraftChange(value);
  const onApply = (): void => props.onApply();
  const onClear = (): void => props.onClear();

  const resolvedTypeKey = $derived.by(
    () => resolveDataTypeKey(entry.dataType ?? entry.baseValue?.dataType ?? entry.overrideValue?.dataType) ?? 'string'
  );
  const resolvedEntryType = $derived.by<PipelineDataType | string>(() => entry.dataType ?? entry.baseValue?.dataType ?? 'generic');
  const sliderMin = $derived.by(() => entry.metadata?.uiMin ?? entry.metadata?.min);
  const sliderMax = $derived.by(() => entry.metadata?.uiMax ?? entry.metadata?.max);
  const sliderStep = $derived.by(() => entry.metadata?.uiStep ?? entry.metadata?.step ?? (['int', 'uint', 'sint'].includes(resolvedTypeKey) ? 1 : undefined));
  const uiControl = $derived.by(() => entry.metadata?.uiControl?.trim().toLowerCase());
  const hasOverride = $derived.by(() => Boolean(entry.overrideValue));
  const isPixel = $derived.by(() => isPixelTypeKey(resolvedTypeKey ?? null));
  const isNumeric = $derived.by(() => isNumericTypeKey(resolvedTypeKey));
  const enumLabelDelimiter = '\u00b7';
  const runtimeTokens = new SvelteSet(['CPU', 'TPU', 'CORAL', 'EDGE TPU', 'EDGE-TPU']);
  const precisionTokens = new SvelteSet(['INT8', 'UINT8', 'F16', 'F32', 'FLOAT16', 'FLOAT32']);

  const isModelIdEntry = $derived.by(() => {
    const portKey = entry.port?.trim().toLowerCase();
    const entryKey = entry.key?.trim().toLowerCase();
    return portKey === 'model_id' || entryKey === 'model_id';
  });

  const resolveEnumVariant = (raw: string, variants: string[]): string | null => {
    if (!variants.length) return null;
    const trimmed = raw?.trim() ?? '';
    if (!trimmed) return variants[0] ?? null;
    const normalized = trimmed.toLowerCase();
    for (const variant of variants) {
      const parsed = parseEnumVariant(variant);
      if (variant.toLowerCase() === normalized) return variant;
      if (parsed.label.toLowerCase() === normalized) return variant;
      if (parsed.value.toLowerCase() === normalized) return variant;
    }
    return variants[0] ?? null;
  };

  const normalizeRuntime = (token: string): string | null => {
    const normalized = token.trim().toUpperCase();
    if (!normalized) return null;
    if (normalized.includes('TPU') || normalized.includes('CORAL')) return 'TPU';
    if (normalized.includes('CPU')) return 'CPU';
    return normalized;
  };

  const normalizePrecision = (token: string): string | null => {
    const normalized = token.trim().toUpperCase();
    if (!normalized) return null;
    if (normalized === 'FLOAT16') return 'F16';
    if (normalized === 'FLOAT32') return 'F32';
    return normalized;
  };

  const modelSelectionProfile = $derived.by(() => {
    if (!isModelIdEntry) return null;
    const variant = resolveEnumVariant(draftValue, entry.variants);
    if (!variant) return null;
    const { label } = parseEnumVariant(variant);
    const tokens = label
      .split(enumLabelDelimiter)
      .map((token) => token.trim())
      .filter(Boolean);
    if (!tokens.length) return null;
    let runtime: string | null = null;
    let precision: string | null = null;
    let cursor = 0;
    if (tokens[cursor] && runtimeTokens.has(tokens[cursor].toUpperCase())) {
      runtime = normalizeRuntime(tokens[cursor]);
      cursor += 1;
    }
    if (tokens[cursor] && precisionTokens.has(tokens[cursor].toUpperCase())) {
      precision = normalizePrecision(tokens[cursor]);
    }
    if (!runtime && !precision) return null;
    return { runtime, precision };
  });
</script>

<div class="space-y-2 rounded border border-surface-800/70 bg-surface-900/40 p-3">
  <div class="flex items-center justify-between gap-3">
    <div>
      <p class="text-sm font-semibold text-white">{entry.port}</p>
      <p class="text-[0.7rem] text-surface-400">
        {describePortType(resolvedEntryType, typePalette)}
      </p>
    </div>
    {#if hasOverride}
      <span class="rounded-full border border-primary-500/50 bg-primary-500/10 px-2 py-[2px] text-micro uppercase tracking-[0.3em] text-primary-200">
        Override
      </span>
    {/if}
  </div>
  <div class="text-micro text-surface-500 space-y-1">
    <p>
      Default:
      <span class="text-surface-200">{formatPipelineValue(entry.baseValue) || '—'}</span>
    </p>
    {#if hasOverride}
      <p>
        Applied:
        <span class="text-primary-200">{formatPipelineValue(entry.overrideValue) || '—'}</span>
      </p>
    {/if}
  </div>
  {#if entry.settable}
    <label class="flex flex-col gap-1 text-micro uppercase tracking-[0.3em] text-surface-500">
      Value
      {#if entry.variants.length > 0}
        <select class="input h-9 text-xs" value={draftValue || entry.variants[0] || ''} onchange={(event) => onDraftChange(event.currentTarget.value)} disabled={constantsReadOnly}>
          {#each entry.variants as variant (variant)}
            {@const parsedVariant = parseEnumVariant(variant)}
            <option value={variant} title={parsedVariant.value}>{parsedVariant.label}</option>
          {/each}
        </select>
        {#if modelSelectionProfile}
          <div class="mt-2 flex flex-wrap gap-2 text-micro-tight uppercase tracking-[0.3em] text-surface-400">
            {#if modelSelectionProfile.runtime}
              <span class="rounded-full border border-surface-700/70 bg-surface-900/70 px-2 py-[2px] text-surface-200">
                {modelSelectionProfile.runtime}
              </span>
            {/if}
            {#if modelSelectionProfile.precision}
              <span class="rounded-full border border-surface-700/70 bg-surface-900/70 px-2 py-[2px] text-surface-200">
                {modelSelectionProfile.precision}
              </span>
            {/if}
          </div>
        {/if}
      {:else if isPixel}
        <div class="flex items-center gap-2">
          <input
            class="h-9 w-16 rounded border border-surface-700 bg-surface-900/70"
            type="color"
            value={draftValue || '#ffffff'}
            oninput={(event) => onDraftChange(event.currentTarget.value)}
            disabled={constantsReadOnly}
          />
          <ColorDropperButton
            title="Pick color from screen"
            ariaLabel="Pick color from screen"
            disabled={constantsReadOnly}
            onPick={onDraftChange}
          />
        </div>
      {:else if isNumeric && uiControl !== 'input' && sliderMin !== undefined && sliderMax !== undefined}
        <div class="flex items-center gap-3">
          <input
            class="range-input w-full"
            type="range"
            min={sliderMin}
            max={sliderMax}
            step={sliderStep}
            value={draftValue || sliderMin}
            oninput={(event) => onDraftChange(event.currentTarget.value)}
            disabled={constantsReadOnly}
          />
          <input
            class="input h-9 w-24 text-xs"
            type="number"
            placeholder="Value"
            value={draftValue}
            oninput={(event) => onDraftChange(event.currentTarget.value)}
            disabled={constantsReadOnly}
          />
        </div>
      {:else if isNumeric}
        <input
          class="input h-9 text-xs"
          type="number"
          placeholder="Enter value"
          value={draftValue}
          oninput={(event) => onDraftChange(event.currentTarget.value)}
          disabled={constantsReadOnly}
        />
      {:else}
        <input
          class="input h-9 text-xs"
          placeholder="Enter value"
          value={draftValue}
          oninput={(event) => onDraftChange(event.currentTarget.value)}
          disabled={constantsReadOnly}
        />
      {/if}
    </label>
    {#if error}
      <p class="text-micro text-error-300">{error}</p>
    {/if}
    <div class="flex flex-wrap gap-2">
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onApply} disabled={constantsReadOnly}>
        Apply
      </button>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClear} disabled={!hasOverride || constantsReadOnly}>
        Clear
      </button>
    </div>
  {:else}
    <p class="text-micro text-surface-500">This port is not settable.</p>
  {/if}
</div>
