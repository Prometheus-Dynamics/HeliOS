<script lang="ts">
  import RangeBandSlider from '$lib/components/controls/RangeBandSlider.svelte';
  import ColorDropperButton from '$lib/components/controls/ColorDropperButton.svelte';
  import { describePortType } from '$lib/features/pipelines/inspector/inspectorTypeUtils';
  import { formatPipelineValue } from '$lib/features/pipelines/valueFormatting';
  import type { PipelineDataType, PipelineNodeValue, PipelinePortMetadata, PipelineTypeDescriptor } from '$lib/types/pipeline';

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

  const {
    label,
    dataType,
    typePalette,
    minEntry,
    maxEntry,
    sliderMin,
    sliderMax,
    sliderStep,
    minDraft,
    maxDraft,
    rangeGradient,
    hasOverride,
    rangeError,
    constantsReadOnly,
    showDropper,
    onDropperPick,
    onRangeChange,
    onMinInput,
    onMaxInput,
    onApply,
    onClear
  } = $props<{
    label: string;
    dataType: PipelineDataType | string | undefined;
    typePalette: Record<string, PipelineTypeDescriptor>;
    minEntry: NodeParameterEntry;
    maxEntry: NodeParameterEntry;
    sliderMin: number;
    sliderMax: number;
    sliderStep?: number;
    minDraft: number;
    maxDraft: number;
    rangeGradient: string;
    hasOverride: boolean;
    rangeError: string | null;
    constantsReadOnly: boolean;
    showDropper: boolean;
    onDropperPick: (hex: string) => void;
    onRangeChange: (payload: { min: number; max: number }) => void;
    onMinInput: (value: string) => void;
    onMaxInput: (value: string) => void;
    onApply: () => void;
    onClear: () => void;
  }>();

</script>

<div class="space-y-2 rounded border border-surface-800/70 bg-surface-900/40 p-3">
  <div class="flex items-center justify-between gap-3">
    <div>
      <p class="text-sm font-semibold text-white">{label}</p>
      <p class="text-[0.7rem] text-surface-400">{describePortType(dataType ?? 'generic', typePalette)}</p>
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
      <span class="text-surface-200">
        {formatPipelineValue(minEntry.baseValue) || '—'} → {formatPipelineValue(maxEntry.baseValue) || '—'}
      </span>
    </p>
    {#if hasOverride}
      <p>
        Applied:
        <span class="text-primary-200">
          {formatPipelineValue(minEntry.overrideValue ?? minEntry.baseValue) || '—'} →
          {formatPipelineValue(maxEntry.overrideValue ?? maxEntry.baseValue) || '—'}
        </span>
      </p>
    {/if}
  </div>
  <label class="flex flex-col gap-2 text-micro uppercase tracking-[0.3em] text-surface-500">
    <div class="flex items-center justify-between gap-2">
      <span>Range</span>
      {#if showDropper}
        <ColorDropperButton
          title={`Pick ${label.toLowerCase()} from screen`}
          ariaLabel={`Pick ${label} from screen`}
          disabled={constantsReadOnly}
          onPick={onDropperPick}
        />
      {/if}
    </div>
    <RangeBandSlider
      min={sliderMin}
      max={sliderMax}
      step={sliderStep}
      valueMin={minDraft}
      valueMax={maxDraft}
      gradient={rangeGradient}
      disabled={constantsReadOnly}
      on:change={(event) => onRangeChange(event.detail)}
    />
    <div class="flex gap-2">
      <input
        class="input h-9 w-24 text-xs"
        type="number"
        placeholder="Min"
        step={sliderStep}
        value={minDraft}
        oninput={(event) => onMinInput((event.currentTarget as HTMLInputElement).value)}
        disabled={constantsReadOnly}
      />
      <input
        class="input h-9 w-24 text-xs"
        type="number"
        placeholder="Max"
        step={sliderStep}
        value={maxDraft}
        oninput={(event) => onMaxInput((event.currentTarget as HTMLInputElement).value)}
        disabled={constantsReadOnly}
      />
    </div>
  </label>
  {#if rangeError}
    <p class="text-micro text-error-300">{rangeError}</p>
  {/if}
  <div class="flex flex-wrap gap-2">
    <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onApply} disabled={constantsReadOnly}>
      Apply
    </button>
    <button
      class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
      type="button"
      onclick={onClear}
      disabled={!hasOverride || constantsReadOnly}
    >
      Clear
    </button>
  </div>
</div>
