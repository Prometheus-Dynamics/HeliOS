<script lang="ts">
  import {
    DEFAULT_PIXEL_COLOR,
    isPixelTypeKey,
    parsePixelDraft,
    parsePixelValue,
    pixelToHex
  } from '$lib/components/flow/pipeline-graph/editorUtils';
  import {
    formatPipelineValue,
    getDataTypeVariants,
    getMetadataEnumOptions,
    isDataTypeSettable,
    resolveDataTypeKey
  } from '$lib/features/pipelines/valueFormatting';
  import ColorDropperButton from '$lib/components/controls/ColorDropperButton.svelte';
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineTuningConstantGroup } from '$lib/components/pipelines/types';
  import { SvelteSet } from 'svelte/reactivity';

  type Props = {
    streamLabel: string;
    streamId: string | null;
    constantSearch: string;
    constantGroups: PipelineTuningConstantGroup[];
    filteredConstantGroups: PipelineTuningConstantGroup[];
    streamNodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
    streamNodeErrors?: Record<string, Record<string, string | null>>;
    streamError?: string | null;
    readNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    onSearch: (value: string) => void;
  };

  const NUMERIC_TYPE_KEYS = new SvelteSet(['uint', 'sint', 'int', 'float', 'double', 'number']);

  let {
    streamLabel,
    streamId,
    constantSearch,
    constantGroups,
    filteredConstantGroups,
    streamNodeOverrides,
    streamNodeErrors = {},
    streamError = null,
    readNodeDraft,
    updateStreamNodeValue,
    onSearch
  }: Props = $props();
</script>

<div class="space-y-3">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <div>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Per stream</p>
      <p class="text-xs text-surface-400">Overrides for {streamLabel}.</p>
    </div>
  </div>

  {#if streamError}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
      {streamError}
    </div>
  {/if}

  <div class="space-y-2">
    <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Constants</p>
    <input
      class="w-full rounded border border-surface-800/70 bg-surface-900/60 px-3 py-2 text-xs text-surface-200"
      type="search"
      placeholder="Search constants…"
      value={constantSearch}
      oninput={(event) => onSearch((event.currentTarget as HTMLInputElement).value)}
    />
    {#if constantGroups.length === 0}
      <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
        No node constants available for this pipeline.
      </div>
    {:else if filteredConstantGroups.length === 0}
      <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
        No constants match your search.
      </div>
    {:else}
      <div class="space-y-3">
        {#each filteredConstantGroups as group (group.nodeId)}
          <div class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
            <div class="min-w-0">
              <p class="truncate text-xs font-semibold text-surface-100">{group.nodeLabel}</p>
              <p class="truncate text-micro-tight text-surface-600">{group.nodeId}</p>
            </div>
            <div class="mt-3 grid gap-2 md:grid-cols-2">
              {#each group.entries as entry (group.nodeId + ':' + entry.portKey)}
                {@const normalizedPort = entry.portKey.trim().toLowerCase()}
                {@const typeKey = resolveDataTypeKey(entry.dataType ?? undefined) ?? 'string'}
                {@const typeKeyLower = typeKey.toLowerCase()}
                {@const variants = getDataTypeVariants(entry.dataType ?? undefined)}
                {@const meta = entry.metadata}
                {@const metadataOptions = getMetadataEnumOptions(meta)}
                {@const allowedValues = metadataOptions.length ? metadataOptions : variants}
                {@const isBool = typeKeyLower === 'bool' || typeKeyLower === 'boolean'}
                {@const isPixel = isPixelTypeKey(typeKeyLower)}
                {@const isNumeric = NUMERIC_TYPE_KEYS.has(typeKeyLower)}
                {@const isSettable = isDataTypeSettable(entry.dataType ?? undefined)}
                {@const baseValue = entry.overrideValue ?? entry.baseValue}
                {@const baseLabel = baseValue ? formatPipelineValue(baseValue) : 'None'}
                {@const overrideValue = streamNodeOverrides?.[group.nodeId]?.[normalizedPort] ?? null}
                {@const hasOverride = Object.prototype.hasOwnProperty.call(streamNodeOverrides?.[group.nodeId] ?? {}, normalizedPort)}
                {@const draftFallback = overrideValue ? formatPipelineValue(overrideValue) : baseLabel === 'None' ? '' : baseLabel}
                {@const draftValue = readNodeDraft(group.nodeId, normalizedPort) ?? draftFallback}
                {@const error = streamNodeErrors?.[group.nodeId]?.[normalizedPort] ?? null}
                {@const pixelValue = parsePixelDraft(draftValue) ?? parsePixelValue(overrideValue ?? baseValue) ?? DEFAULT_PIXEL_COLOR}
                {@const pixelHex = pixelToHex(pixelValue)}
                {@const numericFallback =
                  typeof overrideValue?.value === 'number' && Number.isFinite(overrideValue?.value)
                    ? (overrideValue?.value as number)
                    : typeof baseValue?.value === 'number' && Number.isFinite(baseValue?.value)
                      ? (baseValue?.value as number)
                      : (meta?.uiMin as number | undefined) ?? (meta?.min as number | undefined) ?? 0}
                {@const sliderValue = Number.isFinite(Number(draftValue)) ? Number(draftValue) : numericFallback}
                {@const fallbackStep = typeKeyLower === 'float' || typeKeyLower === 'double' || typeKeyLower === 'number' ? 0.1 : 1}
                {@const numericStep =
                  typeof meta?.uiStep === 'number' && Number.isFinite(meta.uiStep) && meta.uiStep > 0
                    ? meta.uiStep
                    : typeof meta?.step === 'number' && Number.isFinite(meta.step) && meta.step > 0
                      ? meta.step
                      : fallbackStep}
                {@const hasNumericBounds = meta?.uiMin !== undefined && meta?.uiMax !== undefined ? true : meta?.min !== undefined && meta?.max !== undefined}
                <div class="rounded border border-surface-800/70 bg-surface-950/60 p-3 space-y-2">
                  <div class="flex items-start justify-between gap-3">
                    <div class="min-w-0">
                      <p class="truncate text-xs text-surface-100">{entry.portKey}</p>
                      {#if meta?.description}
                        <p class="mt-0.5 line-clamp-2 text-micro-tight text-surface-500">{meta.description as string}</p>
                      {/if}
                    </div>
                    {#if hasOverride}
                      <button
                        class="btn btn-3xs preset-outline"
                        type="button"
                        onclick={() => updateStreamNodeValue(group.nodeId, entry.portKey, entry.dataType, '')}
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
                        updateStreamNodeValue(
                          group.nodeId,
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
                          updateStreamNodeValue(
                            group.nodeId,
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
                        class="h-9 w-16 rounded border border-surface-700 bg-surface-900/70"
                        type="color"
                        value={pixelHex}
                        disabled={!isSettable}
                        oninput={(event) =>
                          updateStreamNodeValue(
                            group.nodeId,
                            entry.portKey,
                            entry.dataType,
                            (event.currentTarget as HTMLInputElement).value
                          )
                        }
                      />
                      <ColorDropperButton
                        title="Pick color from screen"
                        ariaLabel="Pick color from screen"
                        disabled={!isSettable}
                        onPick={(hex) => updateStreamNodeValue(group.nodeId, entry.portKey, entry.dataType, hex)}
                      />
                      <input
                        class="flex-1 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                        value={pixelHex}
                        disabled={!isSettable}
                        oninput={(event) =>
                          updateStreamNodeValue(
                            group.nodeId,
                            entry.portKey,
                            entry.dataType,
                            (event.currentTarget as HTMLInputElement).value
                          )
                        }
                      />
                    </div>
                  {:else if isNumeric && hasNumericBounds}
                    <div class="flex items-center gap-2">
                      <input
                        class="range-input w-full"
                        type="range"
                        min={(meta?.uiMin ?? meta?.min) as number}
                        max={(meta?.uiMax ?? meta?.max) as number}
                        step={numericStep}
                        value={sliderValue}
                        disabled={!isSettable}
                        oninput={(event) =>
                          updateStreamNodeValue(
                            group.nodeId,
                            entry.portKey,
                            entry.dataType,
                            (event.currentTarget as HTMLInputElement).value
                          )
                        }
                      />
                      <input
                        class="w-24 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                        type="number"
                        min={(meta?.uiMin ?? meta?.min) as number}
                        max={(meta?.uiMax ?? meta?.max) as number}
                        step={numericStep}
                        value={draftValue}
                        disabled={!isSettable}
                        oninput={(event) =>
                          updateStreamNodeValue(
                            group.nodeId,
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
                      value={draftValue}
                      disabled={!isSettable}
                      placeholder={baseLabel === 'None' ? 'Default' : baseLabel}
                      oninput={(event) =>
                        updateStreamNodeValue(
                          group.nodeId,
                          entry.portKey,
                          entry.dataType,
                          (event.currentTarget as HTMLInputElement).value
                        )
                      }
                    />
                  {/if}
                  <div class="flex flex-wrap gap-2 text-micro-tight text-surface-500">
                    <span>Default: {baseLabel}</span>
                    {#if overrideValue}
                      <span>Override: {formatPipelineValue(overrideValue)}</span>
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
  {#if !streamId}
    <p class="text-micro-tight text-surface-500">No stream selected.</p>
  {/if}
</div>
