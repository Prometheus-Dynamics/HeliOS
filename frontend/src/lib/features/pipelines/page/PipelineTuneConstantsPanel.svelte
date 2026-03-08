<script lang="ts">
  import {
    getDataTypeVariants,
    getMetadataEnumOptions,
    isDataTypeSettable,
    formatPipelineValue,
    resolveDataTypeKey
  } from '$lib/features/pipelines/valueFormatting';
  import {
    DEFAULT_PIXEL_COLOR,
    hexToRgb,
    isPixelTypeKey,
    parsePixelDraft,
    parsePixelValue,
    pixelToHex
  } from '$lib/components/flow/pipeline-graph/editorUtils';
  import type { PipelineDataType, PipelineGraphPlan, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineTuningConstantGroup } from '$lib/components/pipelines/types';
  import { SvelteSet } from 'svelte/reactivity';

  type Props = {
    tuneConstantSearch: string;
    tuneConstantGroups: PipelineTuningConstantGroup[];
    tuneFilteredConstantGroups: PipelineTuningConstantGroup[];
    tuneNodeErrors: Record<string, Record<string, string | null>>;
    tunePlan: PipelineGraphPlan | null | undefined;
    isDaedalusPlan: (plan: PipelineGraphPlan | null | undefined) => boolean;
    safeClonePlan: (plan: PipelineGraphPlan) => PipelineGraphPlan;
    handlePlanChange: (plan: PipelineGraphPlan) => void;
    normalizePortKey: (value: string) => string;
    readTuneNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateGlobalNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    clearTuneNodeDraft: (nodeId: string, portKey: string) => void;
    setTuneNodeError: (nodeId: string, portKey: string, error: string | null) => void;
    scheduleTuneGlobalAutoSave: () => void;
    setNodeConstantValue: (nodeId: string, portKey: string, value: PipelineNodeValue) => void;
    onSearch: (value: string) => void;
  };

  let {
    tuneConstantSearch,
    tuneConstantGroups,
    tuneFilteredConstantGroups,
    tuneNodeErrors,
    tunePlan,
    isDaedalusPlan,
    safeClonePlan,
    handlePlanChange,
    normalizePortKey,
    readTuneNodeDraft,
    updateGlobalNodeValue,
    clearTuneNodeDraft,
    setTuneNodeError,
    scheduleTuneGlobalAutoSave,
    setNodeConstantValue,
    onSearch
  }: Props = $props();

  const NUMERIC_TYPE_KEYS = new SvelteSet(['uint', 'sint', 'int', 'float', 'double', 'number']);
  const clearNodeConstantValue = (nodeId: string, portKey: string): void => {
    const clearValue = setNodeConstantValue as unknown as (
      nodeId: string,
      portKey: string,
      value: PipelineNodeValue | null
    ) => void;
    clearValue(nodeId, portKey, null);
  };
</script>

<div class="space-y-4">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <p class="text-xs uppercase tracking-[0.3em] text-surface-500">All consumers</p>
  </div>

  <div class="space-y-2">
    <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Constants</p>
    <input
      class="w-full rounded border border-surface-800/70 bg-surface-900/60 px-3 py-2 text-xs text-surface-200"
      type="search"
      placeholder="Search constants…"
      value={tuneConstantSearch}
      oninput={(event) => onSearch((event.currentTarget as HTMLInputElement).value)}
    />
    {#if tuneConstantGroups.length === 0}
      <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
        No node constants available for this pipeline.
      </div>
    {:else if tuneFilteredConstantGroups.length === 0}
      <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
        No constants match your search.
      </div>
    {:else}
      <div class="space-y-3">
        {#each tuneFilteredConstantGroups as group (group.nodeId)}
          <div class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
            <div class="min-w-0">
              <p class="truncate text-xs font-semibold text-surface-100">{group.nodeLabel}</p>
              <p class="truncate text-micro-tight text-surface-600">{group.nodeId}</p>
            </div>
            <div class="mt-3 grid gap-2 md:grid-cols-2">
              {#each group.entries as entry (group.nodeId + ':' + entry.portKey)}
                {@const normalizedPort = normalizePortKey(entry.portKey)}
                {@const typeKey = resolveDataTypeKey(entry.dataType ?? undefined) ?? 'string'}
                {@const typeKeyLower = typeKey.toLowerCase()}
                {@const variants = getDataTypeVariants(entry.dataType ?? undefined)}
                {@const metadataOptions = getMetadataEnumOptions(entry.metadata)}
                {@const allowedValues = metadataOptions.length ? metadataOptions : variants}
                {@const isBool = typeKeyLower === 'bool' || typeKeyLower === 'boolean'}
                {@const isPixel = isPixelTypeKey(typeKeyLower)}
                {@const isNumeric = NUMERIC_TYPE_KEYS.has(typeKeyLower)}
                {@const isSettable = isDataTypeSettable(entry.dataType ?? undefined)}
                {@const defaultLabel = entry.baseValue ? formatPipelineValue(entry.baseValue) : 'None'}
                {@const overrideLabel = entry.overrideValue ? formatPipelineValue(entry.overrideValue) : null}
                {@const draftFallback = entry.overrideValue ? formatPipelineValue(entry.overrideValue) : defaultLabel === 'None' ? '' : defaultLabel}
                {@const draftValue = readTuneNodeDraft(entry.nodeId, normalizedPort) ?? draftFallback}
                {@const error = tuneNodeErrors?.[entry.nodeId]?.[normalizedPort] ?? null}
                {@const pixelValue =
                  parsePixelDraft(draftValue) ?? parsePixelValue(entry.overrideValue ?? entry.baseValue) ?? DEFAULT_PIXEL_COLOR}
                {@const pixelHex = pixelToHex(pixelValue)}
                {@const numericFallback =
                  typeof entry.overrideValue?.value === 'number' && Number.isFinite(entry.overrideValue?.value)
                    ? (entry.overrideValue?.value as number)
                    : typeof entry.baseValue?.value === 'number' && Number.isFinite(entry.baseValue?.value)
                      ? (entry.baseValue?.value as number)
                      : entry.metadata?.uiMin ?? entry.metadata?.min ?? 0}
                {@const sliderValue = Number.isFinite(Number(draftValue)) ? Number(draftValue) : numericFallback}
                {@const fallbackStep = typeKeyLower === 'float' || typeKeyLower === 'double' || typeKeyLower === 'number' ? 0.1 : 1}
                {@const numericStep =
                  typeof entry.metadata?.uiStep === 'number' && Number.isFinite(entry.metadata.uiStep) && entry.metadata.uiStep > 0
                    ? entry.metadata.uiStep
                    : typeof entry.metadata?.step === 'number' && Number.isFinite(entry.metadata.step) && entry.metadata.step > 0
                      ? entry.metadata.step
                      : fallbackStep}
                {@const hasNumericBounds =
                  entry.metadata?.uiMin !== undefined && entry.metadata?.uiMax !== undefined
                    ? true
                    : entry.metadata?.min !== undefined && entry.metadata?.max !== undefined}
                <div class="rounded border border-surface-800/70 bg-surface-950/60 p-3 space-y-2">
                  <div class="flex items-start justify-between gap-3">
                    <div class="min-w-0">
                      <p class="truncate text-xs text-surface-100">{entry.portKey}</p>
                      {#if entry.metadata?.description}
                        <p class="mt-0.5 line-clamp-2 text-micro-tight text-surface-500">{entry.metadata.description}</p>
                      {/if}
                    </div>
                    {#if entry.overrideValue}
                      <button
                        class="btn btn-3xs preset-outline"
                        type="button"
                        onclick={() => {
                          if (tunePlan && isDaedalusPlan(tunePlan)) {
                            const next = safeClonePlan(tunePlan);
                            const node = next.nodes?.[entry.nodeId];
                            if (node) {
                              const existing = node.info?.values ?? {};
                              const nextValues = { ...existing };
                              const existingKey =
                                Object.keys(existing).find((key) => normalizePortKey(key) === normalizedPort) ?? entry.portKey;
                              delete nextValues[existingKey];
                              node.info = { ...node.info, values: nextValues };
                              handlePlanChange(next);
                            }
                          } else {
                            clearNodeConstantValue(entry.nodeId, entry.portKey);
                          }
                          clearTuneNodeDraft(entry.nodeId, normalizedPort);
                          setTuneNodeError(entry.nodeId, normalizedPort, null);
                          scheduleTuneGlobalAutoSave();
                        }}
                      >
                        Reset
                      </button>
                    {/if}
                  </div>
                  {#if allowedValues.length > 0}
                    <select
                      class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      value={draftValue}
                      disabled={!isSettable}
                      onchange={(event) =>
                        updateGlobalNodeValue(
                          entry.nodeId,
                          entry.portKey,
                          entry.dataType,
                          (event.currentTarget as HTMLSelectElement).value
                        )
                      }
                    >
                      {#each allowedValues as option (option)}
                        <option value={option}>{option}</option>
                      {/each}
                    </select>
                  {:else if isBool}
                    <label class="flex items-center gap-2 text-xs text-surface-200">
                      <input
                        type="checkbox"
                        checked={draftValue === 'true'}
                        disabled={!isSettable}
                        onchange={(event) =>
                          updateGlobalNodeValue(
                            entry.nodeId,
                            entry.portKey,
                            entry.dataType,
                            (event.currentTarget as HTMLInputElement).checked ? 'true' : 'false'
                          )
                        }
                      />
                      <span>{draftValue === 'true' ? 'Enabled' : 'Disabled'}</span>
                    </label>
                  {:else if isPixel}
                    <div class="flex items-center gap-2">
                      <input
                        class="h-8 w-10 rounded border border-surface-700 bg-surface-900/70"
                        type="color"
                        value={pixelHex}
                        disabled={!isSettable}
                        oninput={(event) => {
                          const hex = (event.currentTarget as HTMLInputElement).value;
                          const rgb = hexToRgb(hex);
                          if (!rgb) return;
                          const next = JSON.stringify({ r: rgb.r, g: rgb.g, b: rgb.b, a: pixelValue.a ?? 255 });
                          updateGlobalNodeValue(entry.nodeId, entry.portKey, entry.dataType, next);
                        }}
                      />
                      <input
                        class="flex-1 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                        value={draftValue}
                        disabled={!isSettable}
                        placeholder={defaultLabel === 'None' ? 'Default' : defaultLabel}
                        oninput={(event) =>
                          updateGlobalNodeValue(
                            entry.nodeId,
                            entry.portKey,
                            entry.dataType,
                            (event.currentTarget as HTMLInputElement).value
                          )
                        }
                      />
                    </div>
                  {:else if isNumeric}
                    {#if hasNumericBounds}
                      <div class="flex items-center gap-2">
                        <input
                          class="flex-1"
                          type="range"
                          min={entry.metadata?.uiMin ?? entry.metadata?.min}
                          max={entry.metadata?.uiMax ?? entry.metadata?.max}
                          step={numericStep}
                          value={sliderValue}
                          disabled={!isSettable}
                          oninput={(event) =>
                            updateGlobalNodeValue(
                              entry.nodeId,
                              entry.portKey,
                              entry.dataType,
                              (event.currentTarget as HTMLInputElement).value
                            )
                          }
                        />
                        <input
                          class="w-24 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                          type="number"
                          min={entry.metadata?.uiMin ?? entry.metadata?.min}
                          max={entry.metadata?.uiMax ?? entry.metadata?.max}
                          step={numericStep}
                          value={draftValue === '' ? String(sliderValue) : draftValue}
                          disabled={!isSettable}
                          oninput={(event) =>
                            updateGlobalNodeValue(
                              entry.nodeId,
                              entry.portKey,
                              entry.dataType,
                              (event.currentTarget as HTMLInputElement).value
                            )
                          }
                        />
                      </div>
                    {:else}
                      <input
                        class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                        type="number"
                        step={numericStep}
                        value={draftValue === '' ? String(sliderValue) : draftValue}
                        disabled={!isSettable}
                        placeholder={defaultLabel === 'None' ? 'Default' : defaultLabel}
                        oninput={(event) =>
                          updateGlobalNodeValue(
                            entry.nodeId,
                            entry.portKey,
                            entry.dataType,
                            (event.currentTarget as HTMLInputElement).value
                          )
                        }
                      />
                    {/if}
                  {:else}
                    <input
                      class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                      value={draftValue}
                      disabled={!isSettable}
                      placeholder={defaultLabel === 'None' ? 'Default' : defaultLabel}
                      oninput={(event) =>
                        updateGlobalNodeValue(
                          entry.nodeId,
                          entry.portKey,
                          entry.dataType,
                          (event.currentTarget as HTMLInputElement).value
                        )
                      }
                    />
                  {/if}
                  <div class="flex flex-wrap gap-2 text-micro-tight text-surface-500">
                    <span>Default: {defaultLabel}</span>
                    {#if overrideLabel}
                      <span>Override: {overrideLabel}</span>
                    {/if}
                  </div>
                  {#if !isSettable}
                    <p class="text-micro-tight text-surface-600">This constant is not settable.</p>
                  {/if}
                  {#if error}
                    <p class="text-micro-tight text-error-300">{error}</p>
                  {/if}
                </div>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
