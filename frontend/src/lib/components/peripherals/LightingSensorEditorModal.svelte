<script lang="ts">
  import { createEventDispatcher, onDestroy, onMount, untrack } from 'svelte';

  import type { PeripheralEntry } from '$lib/types/devices';
  import SensorModalShell from './SensorModalShell.svelte';
  import LightingAdvancedModal from './lighting/LightingAdvancedModal.svelte';
  import LightingSensorEditorModalContent from './lighting/LightingSensorEditorModalContent.svelte';
  import { createSettingsLoader } from '$lib/components/peripherals/useSensorModalState';
  import {
    clampNumber,
    normalizeLighting,
    type LightingAnimationTemplateSummary,
    type LightingRuntimeState,
    type SavedLightingAnimation,
    type TimelineKeyframe
  } from './lighting/lightingModalUtils';
  import { sampleTimelineFrameAt } from './lighting/lightingTimelineState';
  import {
    formatRuntimeAnimationLabel,
    normalizedAnimationKey,
    runtimeAnimationKind
  } from './lighting/lightingAnimationLibrary';
  import {
    fetchLightingRuntimeState,
    openLightingStateSocket,
  } from './lighting/lightingModalApi';
  import {
    createLightingEditorController,
    type LightingEditorControllerState
  } from './lighting/lightingEditorController';
  import { createLightingEditorPersistence } from './lighting/lightingEditorPersistence';
  import {
    deviceSettingsStore,
    type DeviceSettingsState
  } from '../../../routes/settings/deviceSettingsStore';
  import type { LedConfig } from '../../../routes/settings/types';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import { SvelteSet } from 'svelte/reactivity';

  type Props = {
    peripheral: PeripheralEntry;
    onClose: () => void;
    onRefresh: () => void;
  };

  type EditorStateSnapshot = {
    ledColors: string[];
    ledWhites: number[];
    ledSelectionBrightness: number[];
    selectedLedIndices: number[];
    editorColor: string;
    editorBrightness: number;
  };

  const { peripheral, onClose, onRefresh }: Props = $props();
  const dispatch = createEventDispatcher<{ close: void }>();

  const MIN_FREQ_KHZ = 1;
  const MAX_FREQ_KHZ = 2_000;
  const SAVED_REF_PREFIX = 'saved:';
  const TEMPLATE_REF_PREFIX = 'template:';
  const DEFAULT_ANIMATION_EVENT_OPTIONS = [
    { key: 'startup', label: 'Startup' },
    { key: 'startup_idle', label: 'Startup idle' },
    { key: 'reboot', label: 'Reboot' },
    { key: 'update', label: 'Update' },
    { key: 'update_error', label: 'Update error' },
    { key: 'engine_crash', label: 'Engine crash' }
  ] as const;
  const BUILTIN_TEMPLATE_NAMES = [
    'All Off',
    'Static Warm White',
    'Status Green Ring',
    'Cyan Chase',
    'Magenta Pulse',
    'Rainbow Fast',
    'Breathing Rainbow Slow',
    'Sunrise Timeline',
    'Split Red Blue Timeline',
    'Color Wipe Sequence'
  ] as const;
  const DEFAULT_LIGHTING: LedConfig = {
    enabled: true,
    gpio: 13,
    count: 16,
    use_pwm: true,
    color_order: 'rgb',
    frequency_hz: 800_000,
    brightness: 128,
    label: 'Status ring',
    protocol: 'sk6812-ec20',
    default_animations: {}
  };

  const deviceState = $derived($deviceSettingsStore as DeviceSettingsState);

  let form = $state<LedConfig>({ ...DEFAULT_LIGHTING });
  let lightingCount = $state<number>(DEFAULT_LIGHTING.count);
  let ledColors = $state<string[]>(
    Array.from({ length: DEFAULT_LIGHTING.count }, () => '#00c8ff')
  );
  let ledWhites = $state<number[]>(
    Array.from({ length: DEFAULT_LIGHTING.count }, () => 0)
  );
  let selectedLedIndices = $state<number[]>([0]);
  let ledSelectionBrightness = $state<number[]>(
    Array.from({ length: DEFAULT_LIGHTING.count }, () => 255)
  );
  let editorColor = $state('#00c8ff');
  let editorBrightness = $state(255);
  let liveBrightness = $state<number>(DEFAULT_LIGHTING.brightness ?? 128);
  let frequencyKhz = $state<number>(
    Math.round(DEFAULT_LIGHTING.frequency_hz / 1000)
  );
  let frequencyKhzTouched = $state(false);
  let timelineKeyframes = $state<TimelineKeyframe[]>([]);
  let timelineCursorMs = $state(0);
  let timelineDurationMs = $state(1800);
  let timelineSampleMs = $state(50);
  let timelineLoop = $state(false);
  let selectedTimelineKeyframeId = $state<string | null>(null);
  let sequenceBusy = $state(false);
  let sequenceError = $state<string | null>(null);
  let sequenceToken = $state(0);
  let previewPrimed = $state(false);
  let deviceLightingState = $state<LightingRuntimeState | null>(null);
  let editorUndoStack = $state<EditorStateSnapshot[]>([]);
  let editorRedoStack = $state<EditorStateSnapshot[]>([]);

  let status = $state<string | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let liveStatus = $state<string | null>(null);
  let liveError = $state<string | null>(null);
  let liveBusy = $state(false);
  let savedAnimations = $state<SavedLightingAnimation[]>([]);
  let lightingTemplates = $state<LightingAnimationTemplateSummary[]>([]);
  let savedBusy = $state(false);
  let templatesBusy = $state(false);
  let selectedLoadAnimationRef = $state('');
  let animationName = $state('');
  let animationLoadBusy = $state(false);
  let animationSaveBusy = $state(false);
  let animationImportBusy = $state(false);
  let animationFormStatus = $state<string | null>(null);
  let animationFormError = $state<string | null>(null);
  let settingsBusy = $state(false);
  let settingsError = $state<string | null>(null);
  let lastLightingFingerprint = $state<string | null>(null);
  let showAdvanced = $state(false);
  let animationUploadInput: HTMLInputElement | undefined;
  let lightingStateSocketClose: (() => void) | null = null;
  let lightingAnimationTickId: ReturnType<typeof setInterval> | null = null;

  const EDITOR_HISTORY_LIMIT = 120;
  const ensureDeviceSettings = createSettingsLoader({
    load: deviceSettingsStore.load,
    setBusy: (value) => {
      settingsBusy = value;
    },
    setError: (value) => {
      settingsError = value;
    },
    fallback: 'Unable to load lighting settings.'
  });

  const templateNameKeys = $derived(
    new SvelteSet([
      ...BUILTIN_TEMPLATE_NAMES.map((name) => normalizedAnimationKey(name)),
      ...lightingTemplates.map((entry) => normalizedAnimationKey(entry.name))
    ])
  );
  const visibleSavedAnimations = $derived(
    savedAnimations.filter(
      (entry) => !templateNameKeys.has(normalizedAnimationKey(entry.name))
    )
  );
  const fallbackTemplateAnimations = $derived(
    savedAnimations.filter(
      (entry) =>
        templateNameKeys.has(normalizedAnimationKey(entry.name)) &&
        !lightingTemplates.some(
          (template) =>
            normalizedAnimationKey(template.name) ===
            normalizedAnimationKey(entry.name)
        )
    )
  );
  const templateAnimationNameOptions = $derived(
    (() => {
      const names = new SvelteSet<string>();
      for (const template of lightingTemplates) {
        if (template.name.trim().length > 0) {
          names.add(template.name.trim());
        }
      }
      for (const entry of fallbackTemplateAnimations) {
        if (entry.name.trim().length > 0) {
          names.add(entry.name.trim());
        }
      }
      return [...names].sort((a, b) => a.localeCompare(b));
    })()
  );
  const defaultAnimationNameOptions = $derived(
    (() => {
      const names = new SvelteSet<string>();
      for (const entry of savedAnimations) {
        if (entry.name.trim().length > 0) {
          names.add(entry.name.trim());
        }
      }
      for (const templateName of templateAnimationNameOptions) {
        names.add(templateName);
      }
      const configured = form.default_animations ?? {};
      for (const value of Object.values(configured)) {
        if (typeof value === 'string' && value.trim().length > 0) {
          names.add(value.trim());
        }
      }
      return [...names].sort((a, b) => a.localeCompare(b));
    })()
  );

  function isTemplateNamedAnimation(name: string): boolean {
    return templateNameKeys.has(normalizedAnimationKey(name));
  }

  function stopLightingAnimationTicker(): void {
    if (lightingAnimationTickId) {
      clearInterval(lightingAnimationTickId);
      lightingAnimationTickId = null;
    }
  }

  function syncFromRuntimeState(
    state: LightingRuntimeState,
    source: 'http' | 'ws'
  ): void {
    deviceLightingState = state;
    if (typeof state.brightness === 'number' && Number.isFinite(state.brightness)) {
      liveBrightness = clampNumber(state.brightness, 0, 255);
    }

    stopLightingAnimationTicker();
    const animationKind = runtimeAnimationKind(state);
    const animationRunning =
      Boolean(state.animation_running) || animationKind != null;
    if (animationRunning && animationKind) {
      const label = formatRuntimeAnimationLabel(animationKind);
      liveStatus =
        source === 'http'
          ? `Live animation active (${label})`
          : `Device animation active (${label})`;
      return;
    }

    if (Array.isArray(state.frame)) {
      liveStatus =
        source === 'http'
          ? 'Loaded live device frame state'
          : 'Device frame output updated';
      return;
    }
    liveStatus =
      source === 'http'
        ? 'Loaded live device output state'
        : 'Device output updated';
  }

  async function loadLightingRuntimeState(): Promise<void> {
    try {
      const runtimeState = await fetchLightingRuntimeState();
      syncFromRuntimeState(runtimeState, 'http');
    } catch (err) {
      liveError = buildErrorMessage({
        error: err,
        fallback: 'Unable to load live lighting state.'
      });
    }
  }

  function startLightingStateSocket(): void {
    if (lightingStateSocketClose) {
      lightingStateSocketClose();
      lightingStateSocketClose = null;
    }
    lightingStateSocketClose = openLightingStateSocket({
      onState: (runtimeState) => {
        syncFromRuntimeState(runtimeState, 'ws');
      },
      onError: (message) => {
        liveError = message;
      }
    });
  }

  const controllerState: LightingEditorControllerState & Record<string, any> = {
    get form() {
      return form;
    },
    set form(value) {
      form = value;
    },
    get lightingCount() {
      return lightingCount;
    },
    set lightingCount(value) {
      lightingCount = value;
    },
    get ledColors() {
      return ledColors;
    },
    set ledColors(value) {
      ledColors = value;
    },
    get ledWhites() {
      return ledWhites;
    },
    set ledWhites(value) {
      ledWhites = value;
    },
    get selectedLedIndices() {
      return selectedLedIndices;
    },
    set selectedLedIndices(value) {
      selectedLedIndices = value;
    },
    get ledSelectionBrightness() {
      return ledSelectionBrightness;
    },
    set ledSelectionBrightness(value) {
      ledSelectionBrightness = value;
    },
    get editorColor() {
      return editorColor;
    },
    set editorColor(value) {
      editorColor = value;
    },
    get editorBrightness() {
      return editorBrightness;
    },
    set editorBrightness(value) {
      editorBrightness = value;
    },
    get liveBrightness() {
      return liveBrightness;
    },
    set liveBrightness(value) {
      liveBrightness = value;
    },
    get timelineKeyframes() {
      return timelineKeyframes;
    },
    set timelineKeyframes(value) {
      timelineKeyframes = value;
    },
    get timelineCursorMs() {
      return timelineCursorMs;
    },
    set timelineCursorMs(value) {
      timelineCursorMs = value;
    },
    get timelineDurationMs() {
      return timelineDurationMs;
    },
    set timelineDurationMs(value) {
      timelineDurationMs = value;
    },
    get timelineSampleMs() {
      return timelineSampleMs;
    },
    set timelineSampleMs(value) {
      timelineSampleMs = value;
    },
    get timelineLoop() {
      return timelineLoop;
    },
    set timelineLoop(value) {
      timelineLoop = value;
    },
    get selectedTimelineKeyframeId() {
      return selectedTimelineKeyframeId;
    },
    set selectedTimelineKeyframeId(value) {
      selectedTimelineKeyframeId = value;
    },
    get sequenceBusy() {
      return sequenceBusy;
    },
    set sequenceBusy(value) {
      sequenceBusy = value;
    },
    get sequenceError() {
      return sequenceError;
    },
    set sequenceError(value) {
      sequenceError = value;
    },
    get sequenceToken() {
      return sequenceToken;
    },
    set sequenceToken(value) {
      sequenceToken = value;
    },
    get previewPrimed() {
      return previewPrimed;
    },
    set previewPrimed(value) {
      previewPrimed = value;
    },
    get liveStatus() {
      return liveStatus;
    },
    set liveStatus(value) {
      liveStatus = value;
    },
    get liveError() {
      return liveError;
    },
    set liveError(value) {
      liveError = value;
    },
    get liveBusy() {
      return liveBusy;
    },
    set liveBusy(value) {
      liveBusy = value;
    },
    get editorUndoStack() {
      return editorUndoStack;
    },
    set editorUndoStack(value) {
      editorUndoStack = value;
    },
    get editorRedoStack() {
      return editorRedoStack;
    },
    set editorRedoStack(value) {
      editorRedoStack = value;
    }
  };

  const editorController = createLightingEditorController(controllerState, {
    defaultLightingCount: DEFAULT_LIGHTING.count,
    minFreqKhz: MIN_FREQ_KHZ,
    maxFreqKhz: MAX_FREQ_KHZ,
    editorHistoryLimit: EDITOR_HISTORY_LIMIT,
    syncFromRuntimeState
  });

  const {
    addTimelineKeyframe,
    applyAnimationEntryToEditor,
    applyEditorFrameTransient,
    buildCurrentFramePayloadForOutput,
    buildTimelinePayloadForEditor,
    clearLedSelection,
    clearTimelineKeyframes,
    duplicateTimelineKeyframe,
    handleEditorBrightnessChange,
    handleEditorColorChange,
    handleEditorKeydown,
    loadTimelineKeyframe,
    playTimelineOnDevice,
    previewCurrentFrameOnDevice,
    removeTimelineKeyframe,
    selectAllLeds,
    setTimelineCursorMs,
    stopDeviceOutput,
    stopTimelineSequence,
    syncEditorFromSelection,
    toggleLedSelection,
    updateTimelineKeyframeEasing,
    updateTimelineKeyframeTime,
    validate
  } = editorController;

  const handleFrequencyInput = (value: number) => {
    frequencyKhzTouched = true;
    const next = clampNumber(value, MIN_FREQ_KHZ, MAX_FREQ_KHZ);
    frequencyKhz = next;
    form.frequency_hz = Math.round(next * 1000);
  };

  function coerceInt(value: number | string): number {
    const num = typeof value === 'string' ? Number(value) : value;
    return Number.isFinite(num) ? Math.trunc(num) : 0;
  }

  const {
    encodeLoadAnimationRef,
    defaultAnimationRefForEvent,
    deleteSelectedAnimation,
    exportAnimationsPackage,
    handleAnimationUploadInput,
    loadLightingTemplates,
    loadSavedAnimations,
    loadSelectedAnimationIntoEditor,
    resetLightingConfig,
    saveCurrentAnimation,
    saveLighting,
    selectedRefIsSavedEntry,
    setDefaultAnimationRefForEvent,
    triggerAnimationUploadPicker
  } = createLightingEditorPersistence(controllerState, {
    applyAnimationEntryToEditor,
    buildCurrentFramePayloadForOutput,
    buildTimelinePayloadForEditor,
    coerceInt,
    defaultLighting: DEFAULT_LIGHTING,
    isTemplateNamedAnimation,
    normalizeLighting,
    onRefreshRequested: () => onRefresh(),
    savedPrefix: SAVED_REF_PREFIX,
    templatePrefix: TEMPLATE_REF_PREFIX,
    validate
  });

  Object.defineProperties(controllerState, {
    animationFormError: { get: () => animationFormError, set: (value) => (animationFormError = value) },
    animationFormStatus: { get: () => animationFormStatus, set: (value) => (animationFormStatus = value) },
    animationImportBusy: { get: () => animationImportBusy, set: (value) => (animationImportBusy = value) },
    animationLoadBusy: { get: () => animationLoadBusy, set: (value) => (animationLoadBusy = value) },
    animationName: { get: () => animationName, set: (value) => (animationName = value) },
    animationSaveBusy: { get: () => animationSaveBusy, set: (value) => (animationSaveBusy = value) },
    animationUploadInput: { get: () => animationUploadInput, set: (value) => (animationUploadInput = value) },
    deviceLightingState: { get: () => deviceLightingState },
    fallbackTemplateAnimations: { get: () => fallbackTemplateAnimations },
    lightingTemplates: { get: () => lightingTemplates },
    savedBusy: { get: () => savedBusy, set: (value) => (savedBusy = value) },
    selectedLoadAnimationRef: { get: () => selectedLoadAnimationRef, set: (value) => (selectedLoadAnimationRef = value) },
    showAdvanced: { get: () => showAdvanced, set: (value) => (showAdvanced = value) },
    templatesBusy: { get: () => templatesBusy, set: (value) => (templatesBusy = value) },
    visibleSavedAnimations: { get: () => visibleSavedAnimations }
  });

  Object.assign(controllerState, {
    addTimelineKeyframe,
    clearLedSelection,
    clearTimelineKeyframes,
    deleteSelectedAnimation,
    duplicateTimelineKeyframe,
    encodeLoadAnimationRef,
    exportAnimationsPackage,
    handleAnimationUploadInput,
    handleEditorBrightnessChange,
    handleEditorColorChange,
    loadSelectedAnimationIntoEditor,
    loadTimelineKeyframe,
    playTimelineOnDevice,
    previewCurrentFrameOnDevice,
    removeTimelineKeyframe,
    saveCurrentAnimation,
    selectAllLeds,
    selectedRefIsSavedEntry,
    setTimelineCursorMs,
    stopDeviceOutput,
    stopTimelineSequence,
    toggleLedSelection,
    triggerAnimationUploadPicker,
    updateTimelineKeyframeEasing,
    updateTimelineKeyframeTime
  });

  $effect(() => {
    const incoming = normalizeLighting(
      deviceState.data?.lighting ?? undefined,
      DEFAULT_LIGHTING
    );
    const fingerprint = JSON.stringify(incoming);
    if (fingerprint !== lastLightingFingerprint) {
      form = incoming;
      lightingCount = incoming.count || DEFAULT_LIGHTING.count;
      frequencyKhzTouched = false;
      lastLightingFingerprint = fingerprint;
    }
  });

  $effect(() => {
    const next =
      typeof form.brightness === 'number' && Number.isFinite(form.brightness)
        ? form.brightness
        : DEFAULT_LIGHTING.brightness ?? 128;
    liveBrightness = clampNumber(next, 0, 255);
  });

  $effect(() => {
    if (frequencyKhzTouched) return;
    const hz =
      typeof form.frequency_hz === 'number' && Number.isFinite(form.frequency_hz)
        ? form.frequency_hz
        : DEFAULT_LIGHTING.frequency_hz;
    frequencyKhz = clampNumber(
      Math.round(hz / 1000),
      MIN_FREQ_KHZ,
      MAX_FREQ_KHZ
    );
  });

  $effect(() => {
    const count = lightingCount || DEFAULT_LIGHTING.count;
    if (ledColors.length !== count) {
      ledColors = Array.from(
        { length: count },
        (_, idx) => ledColors[idx] ?? '#00c8ff'
      );
    }
    if (ledWhites.length !== count) {
      ledWhites = Array.from({ length: count }, (_, idx) =>
        clampNumber(ledWhites[idx] ?? 0, 0, 255)
      );
    }
    if (ledSelectionBrightness.length !== count) {
      ledSelectionBrightness = Array.from({ length: count }, (_, idx) =>
        clampNumber(ledSelectionBrightness[idx] ?? 255, 0, 255)
      );
    }
    const nextSelection = Array.from(
      new SvelteSet(
        selectedLedIndices
          .map((index) => clampNumber(index, 0, Math.max(0, count - 1)))
          .filter((index) => index >= 0 && index < count)
      )
    );
    const changed =
      nextSelection.length !== selectedLedIndices.length ||
      nextSelection.some((value, idx) => value !== selectedLedIndices[idx]);
    if (changed) {
      selectedLedIndices = nextSelection;
      syncEditorFromSelection();
    }
  });

  $effect(() => {
    if (sequenceBusy || timelineKeyframes.length === 0) return;
    const sampled = sampleTimelineFrameAt(timelineKeyframes, timelineCursorMs);
    if (sampled) {
      untrack(() => {
        applyEditorFrameTransient(sampled);
      });
    }
  });

  onMount(() => {
    if (!deviceState.initialized) {
      void ensureDeviceSettings();
    }
    void loadSavedAnimations();
    void loadLightingTemplates();
    void loadLightingRuntimeState();
    startLightingStateSocket();
    window.addEventListener('keydown', handleEditorKeydown);
    return () => {
      stopTimelineSequence();
      stopLightingAnimationTicker();
      if (lightingStateSocketClose) {
        lightingStateSocketClose();
        lightingStateSocketClose = null;
      }
      window.removeEventListener('keydown', handleEditorKeydown);
    };
  });

  onDestroy(() => {
    stopTimelineSequence();
    stopLightingAnimationTicker();
    if (lightingStateSocketClose) {
      lightingStateSocketClose();
      lightingStateSocketClose = null;
    }
  });

  function handleClose(): void {
    stopTimelineSequence();
    dispatch('close');
    onClose();
  }
</script>

<SensorModalShell
  {peripheral}
  onClose={handleClose}
  layout="stacked"
  maxWidthClass="max-w-[82rem]"
>
  {#snippet viewer()}
    <LightingSensorEditorModalContent state={controllerState} />
  {/snippet}
</SensorModalShell>

{#if showAdvanced}
  <LightingAdvancedModal
    bind:form
    {settingsBusy}
    {settingsError}
    {busy}
    {status}
    {error}
    minFreqKhz={MIN_FREQ_KHZ}
    maxFreqKhz={MAX_FREQ_KHZ}
    {frequencyKhz}
    defaultCount={DEFAULT_LIGHTING.count}
    defaultAnimationEvents={DEFAULT_ANIMATION_EVENT_OPTIONS}
    {defaultAnimationNameOptions}
    resolveDefaultAnimationForEvent={(eventKey) =>
      defaultAnimationRefForEvent(eventKey)}
    onDefaultAnimationChange={(eventKey, animationNameValue) =>
      setDefaultAnimationRefForEvent(eventKey, animationNameValue)}
    onClose={() => (showAdvanced = false)}
    onReset={() => void resetLightingConfig()}
    onRetryLoad={() => void ensureDeviceSettings()}
    onFrequencyInput={(value) => handleFrequencyInput(value)}
    onCountInput={(value) => (lightingCount = value)}
    onSave={() => void saveLighting()}
  />
{/if}
