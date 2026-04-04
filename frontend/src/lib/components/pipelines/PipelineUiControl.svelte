<script lang="ts">
  import {
    DEFAULT_PIXEL_COLOR,
    isPixelTypeKey,
    parsePixelDraft,
    parsePixelValue,
    pixelToHex
  } from '$lib/components/flow/pipeline-graph/editorUtils';
  import PipelineUiColorControl from '$lib/components/pipelines/controls/PipelineUiColorControl.svelte';
  import PipelineUiHsvControl from '$lib/components/pipelines/controls/PipelineUiHsvControl.svelte';
  import {
    formatPipelineValue,
    getDataTypeVariants,
    getMetadataEnumOptions,
    parseEnumVariant,
    resolveDataTypeKey
  } from '$lib/features/pipelines/valueFormatting';
  import PipelineUiPixelControl from '$lib/components/pipelines/controls/PipelineUiPixelControl.svelte';
  import type { PipelineDataType, PipelineNodeValue, PipelinePortMetadata } from '$lib/types/pipeline';
  import type { PipelineUiControl, PipelineUiControlBind, PipelineUiNodeDescriptor } from '$lib/features/pipelines/pipelineUiTypes';
  import {
    clampNumber,
    hsvToHex,
    isHsvBind,
    isHsvRangeBind,
    isRangeBind,
    normalizeHex,
    normalizeNodePortKey,
    parseBind,
    resolveDescriptor,
    type HsvBind,
    type HsvRangeBind,
    type RangeBind
  } from '$lib/components/pipelines/pipelineUiControlSupport';
  import { SvelteSet } from 'svelte/reactivity';

  type Props = {
    control: PipelineUiControl;
    nodeDescriptors: PipelineUiNodeDescriptor[];
    streamNodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
    streamNodeErrors?: Record<string, Record<string, string | null>>;
    readNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    readLocalValue: (key: string, fallback: string) => string;
    setLocalValue: (key: string, value: string) => void;
  };

  const NUMERIC_TYPE_KEYS = new SvelteSet(['uint', 'sint', 'int', 'float', 'double', 'number']);
  const HEX_COLOR_PATTERN = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;

  let {
    control,
    nodeDescriptors,
    streamNodeOverrides,
    streamNodeErrors = {},
    readNodeDraft,
    updateStreamNodeValue,
    readLocalValue,
    setLocalValue
  }: Props = $props();
  type BoundField = {
    nodeId: string;
    portKey: string;
    dataType: PipelineDataType | null;
    draftValue: string;
  };
  type HsvBindingMap = Partial<Record<string, BoundField>>;

  function buildBoundField(bind: string | undefined | null): BoundField | null {
    if (!bind) return null;
    const descriptor = resolveDescriptor(bind, nodeDescriptors);
    if (!descriptor) return null;
    const nodeId = descriptor.nodeId;
    const portKey = descriptor.portKey;
    const normalizedPort = normalizeNodePortKey(portKey);
    const overrideLookup = findOverrideValue(nodeId, normalizedPort);
    const overrideValue = overrideLookup.value ?? descriptor.overrideValue ?? null;
    const baseValue = descriptor.defaultValue ?? null;
    const baseLabel = baseValue ? formatPipelineValue(baseValue) : 'None';
    const draftFallback = overrideValue ? formatPipelineValue(overrideValue) : baseLabel === 'None' ? '' : baseLabel;
    const draftValue = readNodeDraft(nodeId, normalizedPort) ?? draftFallback;
    return {
      nodeId,
      portKey,
      dataType: descriptor.dataType ?? null,
      draftValue
    };
  }

  function findOverrideValue(
    nodeId: string,
    normalizedPort: string
  ): { value: PipelineNodeValue | null; has: boolean } {
    const nodeOverrides = streamNodeOverrides?.[nodeId];
    if (!nodeOverrides) return { value: null, has: false };
    if (Object.prototype.hasOwnProperty.call(nodeOverrides, normalizedPort)) {
      return { value: nodeOverrides[normalizedPort] ?? null, has: true };
    }
    const fallbackKey = Object.keys(nodeOverrides).find(
      (key) => normalizeNodePortKey(key) === normalizedPort
    );
    if (fallbackKey) {
      return { value: nodeOverrides[fallbackKey] ?? null, has: true };
    }
    return { value: null, has: false };
  }

  function buildHsvBindingMap(
    control: PipelineUiControl,
    bindValue: PipelineUiControlBind | undefined
  ): HsvBindingMap {
    const bindings: HsvBindingMap = {};
    const bindString = typeof bindValue === 'string' ? bindValue.trim() : '';
    const parsedBind = bindString ? parseBind(bindString) : null;
    const baseNodeId = parsedBind?.nodeId ?? (bindString && !bindString.includes('.') ? bindString : '');
    const hsvBind = control.type === 'hsv' && isHsvBind(bindValue) ? bindValue : null;
    const hsvRangeBind = control.type === 'hsv_range' && isHsvRangeBind(bindValue) ? bindValue : null;

    if (control.type === 'hsv') {
      if (hsvBind) {
        const hField = buildBoundField(hsvBind.h);
        const sField = buildBoundField(hsvBind.s);
        const vField = buildBoundField(hsvBind.v);
        if (hField) bindings[`${control.id}:h`] = hField;
        if (sField) bindings[`${control.id}:s`] = sField;
        if (vField) bindings[`${control.id}:v`] = vField;
      } else if (baseNodeId) {
        const hField = buildBoundField(`${baseNodeId}.h`);
        const sField = buildBoundField(`${baseNodeId}.s`);
        const vField = buildBoundField(`${baseNodeId}.v`);
        if (hField) bindings[`${control.id}:h`] = hField;
        if (sField) bindings[`${control.id}:s`] = sField;
        if (vField) bindings[`${control.id}:v`] = vField;
      }
    }

    if (control.type === 'hsv_range') {
      if (hsvRangeBind) {
        const hMinField = buildBoundField(hsvRangeBind.h.min);
        const hMaxField = buildBoundField(hsvRangeBind.h.max);
        const sMinField = buildBoundField(hsvRangeBind.s.min);
        const sMaxField = buildBoundField(hsvRangeBind.s.max);
        const vMinField = buildBoundField(hsvRangeBind.v.min);
        const vMaxField = buildBoundField(hsvRangeBind.v.max);
        if (hMinField) bindings[`${control.id}:hMin`] = hMinField;
        if (hMaxField) bindings[`${control.id}:hMax`] = hMaxField;
        if (sMinField) bindings[`${control.id}:sMin`] = sMinField;
        if (sMaxField) bindings[`${control.id}:sMax`] = sMaxField;
        if (vMinField) bindings[`${control.id}:vMin`] = vMinField;
        if (vMaxField) bindings[`${control.id}:vMax`] = vMaxField;
      } else if (baseNodeId) {
        const hMinField = buildBoundField(`${baseNodeId}.h_min`);
        const hMaxField = buildBoundField(`${baseNodeId}.h_max`);
        const sMinField = buildBoundField(`${baseNodeId}.s_min`);
        const sMaxField = buildBoundField(`${baseNodeId}.s_max`);
        const vMinField = buildBoundField(`${baseNodeId}.v_min`);
        const vMaxField = buildBoundField(`${baseNodeId}.v_max`);
        if (hMinField) bindings[`${control.id}:hMin`] = hMinField;
        if (hMaxField) bindings[`${control.id}:hMax`] = hMaxField;
        if (sMinField) bindings[`${control.id}:sMin`] = sMinField;
        if (sMaxField) bindings[`${control.id}:sMax`] = sMaxField;
        if (vMinField) bindings[`${control.id}:vMin`] = vMinField;
        if (vMaxField) bindings[`${control.id}:vMax`] = vMaxField;
      }
    }

    return bindings;
  }

  function readBoundHsvValue(bindings: HsvBindingMap, key: string, fallback: string): string {
    const bound = bindings[key];
    if (!bound) return readLocalValue(key, fallback);
    const value = bound.draftValue;
    return value === '' ? fallback : value;
  }

  function setBoundHsvValue(bindings: HsvBindingMap, key: string, value: string): void {
    const bound = bindings[key];
    if (!bound) {
      setLocalValue(key, value);
      return;
    }
    updateStreamNodeValue(bound.nodeId, bound.portKey, bound.dataType, value);
  }

  function metadataOrNull(descriptor: PipelineUiNodeDescriptor | null): PipelinePortMetadata | null {
    return descriptor?.metadata ?? null;
  }

  type EnumOption = { raw: string; label: string; value: string };

  function buildEnumOptions(values: ReadonlyArray<string>): EnumOption[] {
    return values.map((entry) => {
      const raw = String(entry ?? '');
      const parsed = parseEnumVariant(raw);
      return {
        raw,
        label: parsed.label,
        value: parsed.value
      };
    });
  }

  function resolveEnumDraftValue(raw: string, options: EnumOption[]): string {
    const trimmed = raw.trim();
    if (!trimmed) return '';
    if (/^-?\d+$/.test(trimmed)) {
      const idx = Number.parseInt(trimmed, 10);
      if (Number.isInteger(idx) && idx >= 0 && idx < options.length) {
        return options[idx]?.value ?? options[idx]?.raw ?? trimmed;
      }
    }
    const lowered = trimmed.toLowerCase();
    for (const option of options) {
      if (option.raw.toLowerCase() === lowered) return option.value;
      if (option.label.toLowerCase() === lowered) return option.value;
      if (option.value.toLowerCase() === lowered) return option.value;
    }
    return trimmed;
  }
</script>

{#if true}
  {@const isDual = control.type === 'dual_slider'}
  {@const isHsvSlider = control.type === 'slider' && (control.id === 'hsv_h' || control.id === 'hsv_s' || control.id === 'hsv_v')}
  {@const bindValue = control.bind}
  {@const hasBinding = typeof bindValue === 'string' && bindValue.trim().length > 0}
  {@const dualBind = isDual && isRangeBind(bindValue) ? bindValue : null}
  {@const dualMinField = dualBind ? buildBoundField(dualBind.min) : null}
  {@const dualMaxField = dualBind ? buildBoundField(dualBind.max) : null}
  {@const hsvBindings = buildHsvBindingMap(control, bindValue)}
  {@const descriptor = resolveDescriptor(bindValue, nodeDescriptors)}
  {@const nodeId = descriptor?.nodeId ?? ''}
  {@const portKey = descriptor?.portKey ?? ''}
  {@const normalizedPort = descriptor ? normalizeNodePortKey(descriptor.portKey) : ''}
  {@const dataType = descriptor?.dataType ?? null}
  {@const typeKey = resolveDataTypeKey(dataType ?? undefined) ?? 'string'}
  {@const typeKeyLower = typeKey.toLowerCase()}
  {@const variants = getDataTypeVariants(dataType ?? undefined)}
  {@const meta = metadataOrNull(descriptor)}
  {@const metadataOptions = getMetadataEnumOptions(meta)}
  {@const allowedValues =
    control.type === 'select' && control.options && control.options.length
      ? control.options
      : metadataOptions.length
        ? metadataOptions
        : variants}
  {@const enumOptions = control.type === 'select' ? buildEnumOptions(allowedValues) : []}
  {@const isBool = typeKeyLower === 'bool' || typeKeyLower === 'boolean'}
  {@const isPixel = isPixelTypeKey(typeKeyLower)}
  {@const isNumeric =
    NUMERIC_TYPE_KEYS.has(typeKeyLower) ||
    control.type === 'slider' ||
    control.type === 'dual_slider' ||
    control.type === 'hsv' ||
    control.type === 'hsv_range'}
  {@const isBound = Boolean(descriptor)}
  {@const isSettable = !hasBinding || isBound}
  {@const baseValue = descriptor?.defaultValue ?? null}
  {@const baseLabel = baseValue ? formatPipelineValue(baseValue) : 'None'}
  {@const overrideLookup = isBound ? findOverrideValue(nodeId, normalizedPort) : { value: null, has: false }}
  {@const overrideValue = isBound ? overrideLookup.value ?? descriptor?.overrideValue ?? null : null}
  {@const hasOverride = isBound ? overrideLookup.has || Boolean(descriptor?.overrideValue) : false}
  {@const controlDefault =
    control.default === undefined || control.default === null
      ? ''
      : Array.isArray(control.default)
        ? control.default.map((value) => String(value))
        : typeof control.default === 'string'
          ? control.default
          : String(control.default)}
  {@const draftFallback = overrideValue ? formatPipelineValue(overrideValue) : baseLabel === 'None' ? '' : baseLabel}
  {@const boundDraftValue = readNodeDraft(nodeId, normalizedPort) ?? draftFallback}
  {@const draftValue = isBound ? boundDraftValue : readLocalValue(control.id, Array.isArray(controlDefault) ? '' : controlDefault)}
  {@const resolvedSelectValue = control.type === 'select' && typeKeyLower === 'enum'
    ? resolveEnumDraftValue(draftValue, enumOptions)
    : draftValue}
  {@const selectValue = control.type === 'select'
    ? (isBound
        ? resolvedSelectValue
        : (resolvedSelectValue || enumOptions[0]?.value || ''))
    : resolvedSelectValue}
  {@const error = isBound ? streamNodeErrors?.[nodeId]?.[normalizedPort] ?? null : null}
  {@const pixelValue =
    parsePixelDraft(draftValue) ?? parsePixelValue(overrideValue ?? baseValue) ?? DEFAULT_PIXEL_COLOR}
  {@const pixelHex = pixelToHex(pixelValue)}
  {@const numericFallback =
    typeof overrideValue?.value === 'number' && Number.isFinite(overrideValue?.value)
      ? (overrideValue?.value)
      : typeof baseValue?.value === 'number' && Number.isFinite(baseValue?.value)
        ? (baseValue?.value)
        : (control.min ?? 0)}
  {@const sliderValue = Number.isFinite(Number(draftValue)) ? Number(draftValue) : numericFallback}
  {@const fallbackStep = typeKeyLower === 'float' || typeKeyLower === 'double' || typeKeyLower === 'number' ? 0.1 : 1}
  {@const numericStep =
    typeof control.step === 'number' && Number.isFinite(control.step) && control.step > 0
      ? control.step
      : fallbackStep}
  {@const trackGradient = control.trackGradient ?? ''}
  {@const dualRangeOverlay = trackGradient
    ? `linear-gradient(90deg, rgba(15, 23, 42, 0.55) 0%, rgba(15, 23, 42, 0.55) var(--dual-min), transparent var(--dual-min), transparent var(--dual-max), rgba(15, 23, 42, 0.55) var(--dual-max), rgba(15, 23, 42, 0.55) 100%)`
    : ''}
  {@const trackFill = control.trackFill ?? ''}
  {@const thumbFill = control.thumbFill ?? '#ffffff'}
  {@const thumbBorder = control.thumbBorder ?? 'rgba(15, 23, 42, 0.9)'}
  {@const thumbBorderWidth =
    typeof control.thumbBorderWidth === 'number' && Number.isFinite(control.thumbBorderWidth)
      ? control.thumbBorderWidth
      : 2}
  {@const defaultTrack = 'linear-gradient(90deg, rgba(148, 163, 184, 0.35), rgba(148, 163, 184, 0.35))'}
  {@const minValue = typeof control.min === 'number' ? control.min : 0}
  {@const maxValue = typeof control.max === 'number' ? control.max : minValue + 1}
  {@const sliderSpan = maxValue - minValue}
  {@const sliderPercent = sliderSpan > 0 ? ((sliderValue - minValue) / sliderSpan) * 100 : 0}
  {@const hsvHue = clampNumber(Number(readLocalValue('hsv_h', '120')), 0, 360)}
  {@const hsvSat = clampNumber(Number(readLocalValue('hsv_s', '0.7')), 0, 1)}
  {@const hsvVal = clampNumber(Number(readLocalValue('hsv_v', '0.8')), 0, 1)}
  {@const hsvSatStart = hsvToHex(hsvHue, 0, hsvVal)}
  {@const hsvSatEnd = hsvToHex(hsvHue, 1, hsvVal)}
  {@const hsvValEnd = hsvToHex(hsvHue, hsvSat, 1)}
  {@const sliderBase = isHsvSlider
    ? control.id === 'hsv_h'
      ? 'linear-gradient(90deg, #ff0000 0%, #ffff00 17%, #00ff00 33%, #00ffff 50%, #0000ff 67%, #ff00ff 83%, #ff0000 100%)'
      : control.id === 'hsv_s'
        ? `linear-gradient(90deg, ${hsvSatStart}, ${hsvSatEnd})`
        : `linear-gradient(90deg, #000000, ${hsvValEnd})`
    : trackGradient
      ? trackGradient
      : defaultTrack}
  {@const sliderFill = trackFill ? trackFill : 'linear-gradient(90deg, var(--color-primary-300, #38bdf8), var(--color-primary-300, #38bdf8))'}
  {@const thumbVars = `--thumb-fill: ${thumbFill}; --thumb-border: ${thumbBorder}; --thumb-border-width: ${thumbBorderWidth}px;`}
  {@const colorFallback = typeof control.default === 'string' ? control.default : '#1f2937'}
  {@const colorValue = normalizeHex(draftValue, colorFallback, HEX_COLOR_PATTERN)}
  {@const colorTextValue = draftValue ? draftValue : colorValue}

  <div class="rounded border border-surface-800/70 bg-surface-950/60 p-3 space-y-2">
  <div class="flex items-start justify-between gap-3">
    <div class="min-w-0">
      <p class="truncate text-xs text-surface-100">{control.label}</p>
      {#if control.help}
        <p class="mt-0.5 line-clamp-2 text-micro-tight text-surface-500">{control.help}</p>
      {/if}
    </div>
    {#if isBound && hasOverride}
      <button
        class="btn btn-3xs preset-outline"
        type="button"
        onclick={() => updateStreamNodeValue(nodeId, portKey, dataType, '')}
      >
        Reset
      </button>
    {/if}
  </div>

  {#if isDual}
    {@const minKey = `${control.id}:min`}
    {@const maxKey = `${control.id}:max`}
    {@const defaultPair = Array.isArray(controlDefault) ? controlDefault : [String(minValue), String(maxValue)]}
    {@const minDraft = dualMinField
      ? (dualMinField.draftValue === '' ? (defaultPair[0] ?? String(minValue)) : dualMinField.draftValue)
      : readLocalValue(minKey, defaultPair[0] ?? String(minValue))}
    {@const maxDraft = dualMaxField
      ? (dualMaxField.draftValue === '' ? (defaultPair[1] ?? String(maxValue)) : dualMaxField.draftValue)
      : readLocalValue(maxKey, defaultPair[1] ?? String(maxValue))}
    {@const minNumber = Number.isFinite(Number(minDraft)) ? Number(minDraft) : minValue}
    {@const maxNumber = Number.isFinite(Number(maxDraft)) ? Number(maxDraft) : maxValue}
    {@const minClamped = clampNumber(Math.min(minNumber, maxNumber), minValue, maxValue)}
    {@const maxClamped = clampNumber(Math.max(minNumber, maxNumber), minValue, maxValue)}
    {@const rangeSpan = maxValue - minValue}
    {@const minPercent = rangeSpan > 0 ? ((minClamped - minValue) / rangeSpan) * 100 : 0}
    {@const maxPercent = rangeSpan > 0 ? ((maxClamped - minValue) / rangeSpan) * 100 : 0}
    {@const setDualMin = (value) => {
      if (dualMinField) {
        updateStreamNodeValue(dualMinField.nodeId, dualMinField.portKey, dualMinField.dataType, value);
      } else {
        setLocalValue(minKey, value);
      }
    }}
    {@const setDualMax = (value) => {
      if (dualMaxField) {
        updateStreamNodeValue(dualMaxField.nodeId, dualMaxField.portKey, dualMaxField.dataType, value);
      } else {
        setLocalValue(maxKey, value);
      }
    }}
    <div class="space-y-2">
      <div class="flex items-center gap-2">
        <input
          class="w-20 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
          type="number"
          min={minValue}
          max={maxValue}
          step={numericStep}
          value={minClamped}
          disabled={!isSettable}
          oninput={(event) => {
            const raw = event.currentTarget.value;
            const next = clampNumber(Number(raw), minValue, maxValue);
            const maxCurrent = Number.isFinite(Number(maxDraft)) ? Number(maxDraft) : maxValue;
            if (next > maxCurrent) {
              setDualMax(String(next));
            }
            setDualMin(String(Math.min(next, maxCurrent)));
          }}
        />
        <div class="dual-range flex-1">
          <div
            class="dual-range-track"
            style={`--dual-min: ${minPercent}%; --dual-max: ${maxPercent}%; ${trackGradient ? `--dual-base: ${trackGradient}; --dual-fill: ${dualRangeOverlay};` : ''}`}
          ></div>
          <input
            class="dual-range-input range-input dual-range-min simple-range"
            type="range"
            min={minValue}
            max={maxValue}
            step={numericStep}
            value={minClamped}
            style={thumbVars}
            disabled={!isSettable}
            oninput={(event) => {
              const raw = event.currentTarget.value;
              const next = clampNumber(Number(raw), minValue, maxValue);
              const maxCurrent = Number.isFinite(Number(maxDraft)) ? Number(maxDraft) : maxValue;
              if (next > maxCurrent) {
                setDualMax(String(next));
              }
              setDualMin(String(Math.min(next, maxCurrent)));
            }}
          />
          <input
            class="dual-range-input range-input dual-range-max simple-range"
            type="range"
            min={minValue}
            max={maxValue}
            step={numericStep}
            value={maxClamped}
            style={thumbVars}
            disabled={!isSettable}
            oninput={(event) => {
              const raw = event.currentTarget.value;
              const next = clampNumber(Number(raw), minValue, maxValue);
              const minCurrent = Number.isFinite(Number(minDraft)) ? Number(minDraft) : minValue;
              if (next < minCurrent) {
                setDualMin(String(next));
              }
              setDualMax(String(Math.max(next, minCurrent)));
            }}
          />
        </div>
        <input
          class="w-20 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
          type="number"
          min={minValue}
          max={maxValue}
          step={numericStep}
          value={maxClamped}
          disabled={!isSettable}
          oninput={(event) => {
            const raw = event.currentTarget.value;
            const next = clampNumber(Number(raw), minValue, maxValue);
            const minCurrent = Number.isFinite(Number(minDraft)) ? Number(minDraft) : minValue;
            if (next < minCurrent) {
              setDualMin(String(next));
            }
            setDualMax(String(Math.max(next, minCurrent)));
          }}
        />
      </div>
    </div>
  {:else if control.type === 'select'}
    <select
      class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
      value={selectValue}
      disabled={!isSettable}
      onchange={(event) => {
        const value = event.currentTarget.value;
        if (isBound) {
          updateStreamNodeValue(nodeId, portKey, dataType, value);
        } else {
          setLocalValue(control.id, value);
        }
      }}
    >
      {#each enumOptions as option (option.raw)}
        <option value={option.value}>{option.label}</option>
      {/each}
    </select>
  {:else if control.type === 'layout_toggle'}
    {@const layoutRows = Math.min(6, Math.max(1, Math.trunc(Number(control.layout?.rows ?? 1))))}
    {@const layoutColumns = Math.min(6, Math.max(1, Math.trunc(Number(control.layout?.columns ?? 1))))}
    {@const layoutSlotCount = Object.values(control.layout?.outputKeys ?? {}).filter((value) => {
      const trimmed = typeof value === 'string' ? value.trim() : '';
      return trimmed.length > 0;
    }).length}
    <div class="space-y-2">
      <label
        class={`flex items-center gap-2 text-xs text-surface-200 ${!isSettable ? 'opacity-60 cursor-not-allowed' : 'cursor-pointer'}`}
      >
        <span class="relative inline-flex h-5 w-9 items-center">
          <input
            type="checkbox"
            class="peer sr-only"
            checked={draftValue === 'true'}
            disabled={!isSettable}
            onchange={(event) => {
              const value = event.currentTarget.checked ? 'true' : 'false';
              setLocalValue(control.id, value);
            }}
          />
          <span class="absolute inset-0 rounded-full bg-surface-700 transition peer-checked:bg-primary-500"></span>
          <span class="absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-white shadow-sm transition peer-checked:translate-x-4"></span>
        </span>
        <span>{draftValue === 'true' ? 'Layout active' : 'Layout inactive'}</span>
      </label>
      <p class="text-micro-tight text-surface-500">
        Layout {layoutRows}×{layoutColumns} · {layoutSlotCount || 0} slot{layoutSlotCount === 1 ? '' : 's'}
      </p>
    </div>
  {:else if control.type === 'toggle' || isBool}
    <label class="flex items-center gap-2 text-xs text-surface-200">
      <input
        type="checkbox"
        checked={draftValue === 'true'}
        disabled={!isSettable}
        onchange={(event) => {
          const value = event.currentTarget.checked ? 'true' : 'false';
          if (isBound) {
            updateStreamNodeValue(nodeId, portKey, dataType, value);
          } else {
            setLocalValue(control.id, value);
          }
        }}
      />
      <span>{draftValue === 'true' ? 'Enabled' : 'Disabled'}</span>
    </label>
  {:else if control.type === 'slider' && isNumeric}
    <div class="flex items-center gap-2">
      <input
        class={`range-input w-full simple-range ${isHsvSlider ? 'hsv-range' : ''}`}
        type="range"
        min={minValue}
        max={maxValue}
        step={numericStep}
        value={sliderValue}
        style={`--range-base: ${sliderBase}; --range-fill: ${sliderFill}; --range-progress: ${sliderPercent}%; ${thumbVars}`}
        disabled={!isSettable}
        oninput={(event) => {
          const value = event.currentTarget.value;
          if (isBound) {
            updateStreamNodeValue(nodeId, portKey, dataType, value);
          } else {
            setLocalValue(control.id, value);
          }
        }}
      />
      <input
        class="w-20 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
        type="number"
        min={minValue}
        max={maxValue}
        step={numericStep}
        value={draftValue}
        disabled={!isSettable}
        oninput={(event) => {
          const value = event.currentTarget.value;
          if (isBound) {
            updateStreamNodeValue(nodeId, portKey, dataType, value);
          } else {
            setLocalValue(control.id, value);
          }
        }}
      />
    </div>
  {:else if control.type === 'hsv' || control.type === 'hsv_range'}
    <PipelineUiHsvControl
      {control}
      {isSettable}
      readLocalValue={(key, fallback) => readBoundHsvValue(hsvBindings, key, fallback)}
      setLocalValue={(key, value) => setBoundHsvValue(hsvBindings, key, value)}
    />
{:else if control.type === 'color'}
    <PipelineUiColorControl
      value={colorValue}
      textValue={colorTextValue}
      disabled={!isSettable}
      onChange={(value) => {
        if (isBound) {
          updateStreamNodeValue(nodeId, portKey, dataType, value);
        } else {
          setLocalValue(control.id, value);
        }
      }}
    />
  {:else if isPixel}
    <PipelineUiPixelControl
      value={pixelHex}
      disabled={!isSettable}
      onChange={(value) => {
        if (isBound) {
          updateStreamNodeValue(nodeId, portKey, dataType, value);
        } else {
          setLocalValue(control.id, value);
        }
      }}
    />
  {:else}
    <input
      class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
      value={draftValue}
      disabled={!isSettable}
      placeholder={baseLabel === 'None' ? 'Default' : baseLabel}
      oninput={(event) => {
        const value = event.currentTarget.value;
        if (isBound) {
          updateStreamNodeValue(nodeId, portKey, dataType, value);
        } else {
          setLocalValue(control.id, value);
        }
      }}
    />
  {/if}

  <div class="flex flex-wrap gap-2 text-micro-tight text-surface-500">
    <span>Default: {baseLabel}</span>
    {#if overrideValue}
      <span>Override: {formatPipelineValue(overrideValue)}</span>
    {/if}
  </div>
  {#if hasBinding && !isBound}
    <p class="text-micro-tight text-amber-300">Binding `{bindValue}` not found in current graph.</p>
  {:else if !isSettable}
    <p class="text-micro-tight text-surface-600">This constant is not settable.</p>
  {/if}
  {#if error}
    <p class="text-micro-tight text-error-300">{error}</p>
  {/if}
  </div>
{/if}

<style>
  @import './PipelineUiControl.css';
</style>
