<script lang="ts">
  import { createEventDispatcher, onDestroy, onMount, untrack } from 'svelte';

  import { toaster } from '$lib';
  import type { PeripheralEntry } from '$lib/types/devices';
  import SensorModalShell from './SensorModalShell.svelte';
  import LightingCompactEditorPanel from './lighting/LightingCompactEditorPanel.svelte';
  import LightingCustomSequencePanel from './lighting/LightingCustomSequencePanel.svelte';
  import LightingAdvancedModal from './lighting/LightingAdvancedModal.svelte';
  import { createSettingsLoader } from '$lib/components/peripherals/useSensorModalState';
  import {
    buildPreviewFrame,
    buildFramePayloadFrom,
    buildTimelinePayloadFromKeyframes,
    clampNumber,
    compileTimelineToFrameSequence,
    normalizeLighting,
    normalizeTimelineKeyframes,
    rgbToHex,
    scaleHex,
    type LightingAnimationTemplateDocument,
    type LightingAnimationTemplateSummary,
    type LightingColorPayload,
    type LightingRuntimeState,
    type LightingTimelineEasing,
    type LightingTimelinePayload,
    type LightingFramePayload,
    type SavedLightingAnimation,
    type TimelineKeyframe
  } from './lighting/lightingModalUtils';
  import {
    deleteLightingAnimation,
    fetchLightingTemplate,
    fetchLightingTemplates,
    fetchLightingRuntimeState,
    fetchSavedAnimations,
    openLightingStateSocket,
    resetLightingConfig as resetLightingConfigApi,
    saveLightingAnimation,
    saveLightingConfig,
    stopLightingOutput
  } from './lighting/lightingModalApi';
  import { deviceSettingsStore, type DeviceSettingsState } from '../../../routes/settings/deviceSettingsStore';
  import type { LightingSettings } from '../../../routes/settings/types';
  import { REQUESTED_BY, apiFetch } from '../../../routes/settings/api';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';

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
  const DEFAULT_LIGHTING: LightingSettings = {
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

  let form = $state<LightingSettings>({ ...DEFAULT_LIGHTING });
  let lightingCount = $state<number>(DEFAULT_LIGHTING.count);
  let ledColors = $state<string[]>(Array.from({ length: DEFAULT_LIGHTING.count }, () => '#00c8ff'));
  let ledWhites = $state<number[]>(Array.from({ length: DEFAULT_LIGHTING.count }, () => 0));
  let selectedLedIndices = $state<number[]>([0]);
  let ledSelectionBrightness = $state<number[]>(Array.from({ length: DEFAULT_LIGHTING.count }, () => 255));
  let editorColor = $state('#00c8ff');
  let editorBrightness = $state(255);
  let liveBrightness = $state<number>(DEFAULT_LIGHTING.brightness ?? 128);
  let frequencyKhz = $state<number>(Math.round(DEFAULT_LIGHTING.frequency_hz / 1000));
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

  function encodeLoadAnimationRef(kind: 'saved' | 'template', value: string): string {
    return `${kind === 'saved' ? SAVED_REF_PREFIX : TEMPLATE_REF_PREFIX}${value}`;
  }

  function decodeLoadAnimationRef(value: string): { kind: 'saved' | 'template'; value: string } | null {
    const trimmed = value.trim();
    if (trimmed.startsWith(SAVED_REF_PREFIX)) {
      return { kind: 'saved', value: trimmed.slice(SAVED_REF_PREFIX.length) };
    }
    if (trimmed.startsWith(TEMPLATE_REF_PREFIX)) {
      return { kind: 'template', value: trimmed.slice(TEMPLATE_REF_PREFIX.length) };
    }
    return null;
  }

  function normalizedAnimationKey(value: string): string {
    return value.trim().toLowerCase();
  }

  const templateNameKeys = $derived(
    new Set([...BUILTIN_TEMPLATE_NAMES.map((name) => normalizedAnimationKey(name)), ...lightingTemplates.map((entry) => normalizedAnimationKey(entry.name))])
  );
  const visibleSavedAnimations = $derived(
    savedAnimations.filter((entry) => !templateNameKeys.has(normalizedAnimationKey(entry.name)))
  );
  const fallbackTemplateAnimations = $derived(
    savedAnimations.filter(
      (entry) =>
        templateNameKeys.has(normalizedAnimationKey(entry.name)) &&
        !lightingTemplates.some((template) => normalizedAnimationKey(template.name) === normalizedAnimationKey(entry.name))
    )
  );
  const templateAnimationNameOptions = $derived(
    (() => {
      const names = new Set<string>();
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
      const names = new Set<string>();
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

  function hasLoadAnimationRef(ref: string): boolean {
    const decoded = decodeLoadAnimationRef(ref);
    if (!decoded) return false;
    if (decoded.kind === 'saved') {
      return visibleSavedAnimations.some((entry) => entry.name === decoded.value);
    }
    const selectedKey = normalizedAnimationKey(decoded.value);
    return (
      lightingTemplates.some((entry) => entry.template_id === decoded.value || normalizedAnimationKey(entry.name) === selectedKey) ||
      fallbackTemplateAnimations.some((entry) => normalizedAnimationKey(entry.name) === selectedKey)
    );
  }

  function defaultLoadAnimationRef(): string {
    if (visibleSavedAnimations.length > 0) {
      return encodeLoadAnimationRef('saved', visibleSavedAnimations[0].name);
    }
    if (lightingTemplates.length > 0) {
      return encodeLoadAnimationRef('template', lightingTemplates[0].template_id);
    }
    if (fallbackTemplateAnimations.length > 0) {
      return encodeLoadAnimationRef('template', fallbackTemplateAnimations[0].name);
    }
    return '';
  }

  function syncLoadAnimationRef(preferred: string = selectedLoadAnimationRef): void {
    selectedLoadAnimationRef = hasLoadAnimationRef(preferred) ? preferred : defaultLoadAnimationRef();
  }

  function selectedRefIsSavedEntry(): boolean {
    const selected = decodeLoadAnimationRef(selectedLoadAnimationRef);
    return Boolean(selected && selected.kind === 'saved' && !isTemplateNamedAnimation(selected.value));
  }

  async function loadSavedAnimations(): Promise<void> {
    savedBusy = true;
    const previousSelection = selectedLoadAnimationRef;
    try {
      savedAnimations = await fetchSavedAnimations(apiFetch);
      syncLoadAnimationRef(previousSelection);
    } catch (err) {
      liveError = buildErrorMessage({ error: err, fallback: 'Unable to load saved animations.' });
      savedAnimations = [];
      syncLoadAnimationRef(previousSelection);
    } finally {
      savedBusy = false;
    }
  }

  async function loadLightingTemplates(): Promise<void> {
    templatesBusy = true;
    const previousSelection = selectedLoadAnimationRef;
    try {
      lightingTemplates = await fetchLightingTemplates(apiFetch);
      syncLoadAnimationRef(previousSelection);
    } catch (err) {
      animationFormError = buildErrorMessage({ error: err, fallback: 'Unable to load built-in lighting templates.' });
      lightingTemplates = [];
      syncLoadAnimationRef(previousSelection);
    } finally {
      templatesBusy = false;
    }
  }

  function stopLightingAnimationTicker(): void {
    if (lightingAnimationTickId) {
      clearInterval(lightingAnimationTickId);
      lightingAnimationTickId = null;
    }
  }

  function runtimeAnimationKind(state: LightingRuntimeState): 'off' | 'chase' | 'pulse' | 'rainbow' | 'breathing_rainbow' | null {
    const kind = state.animation?.kind;
    if (kind === 'off' || kind === 'chase' || kind === 'pulse' || kind === 'rainbow' || kind === 'breathing_rainbow') {
      return kind;
    }
    return null;
  }

  function describeRuntimeState(state: LightingRuntimeState | null): string {
    if (!state) return 'Unknown';
    const kind = runtimeAnimationKind(state);
    if (kind && kind !== 'off') {
      return `Animation: ${kind.replace('_', ' ')}`;
    }
    if (Array.isArray(state.frame)) {
      return `Frame output (${state.frame.length} LEDs)`;
    }
    return 'Idle';
  }

  function formatRuntimeAnimationLabel(kind: Exclude<ReturnType<typeof runtimeAnimationKind>, null>): string {
    return kind.replace('_', ' ');
  }

  function syncFromRuntimeState(state: LightingRuntimeState, source: 'http' | 'ws'): void {
    deviceLightingState = state;
    if (typeof state.brightness === 'number' && Number.isFinite(state.brightness)) {
      liveBrightness = clampNumber(state.brightness, 0, 255);
    }

    stopLightingAnimationTicker();
    const animationKind = runtimeAnimationKind(state);
    const animationRunning = Boolean(state.animation_running) || (animationKind != null && animationKind !== 'off');
    if (animationRunning && animationKind && animationKind !== 'off') {
      const label = formatRuntimeAnimationLabel(animationKind);
      liveStatus = source === 'http' ? `Live animation active (${label})` : `Device animation active (${label})`;
      return;
    }

    if (Array.isArray(state.frame)) {
      liveStatus = source === 'http' ? 'Loaded live device frame state' : 'Device frame output updated';
      return;
    }
    liveStatus = source === 'http' ? 'Loaded live device output state' : 'Device output updated';
  }

  async function loadLightingRuntimeState(): Promise<void> {
    try {
      const state = await fetchLightingRuntimeState(apiFetch);
      syncFromRuntimeState(state, 'http');
    } catch (err) {
      const message = buildErrorMessage({ error: err, fallback: 'Unable to load live lighting state.' });
      liveError = message;
    }
  }

  function startLightingStateSocket(): void {
    if (lightingStateSocketClose) {
      lightingStateSocketClose();
      lightingStateSocketClose = null;
    }
    lightingStateSocketClose = openLightingStateSocket({
      onState: (state) => {
        syncFromRuntimeState(state, 'ws');
      },
      onError: (message) => {
        liveError = message;
      }
    });
  }

  $effect(() => {
    const incoming = normalizeLighting(deviceState.data?.lighting ?? undefined, DEFAULT_LIGHTING);
    const fingerprint = JSON.stringify(incoming);
    if (fingerprint !== lastLightingFingerprint) {
      form = incoming;
      lightingCount = incoming.count || DEFAULT_LIGHTING.count;
      frequencyKhzTouched = false;
      lastLightingFingerprint = fingerprint;
    }
  });

  $effect(() => {
    const next = typeof form.brightness === 'number' && Number.isFinite(form.brightness) ? form.brightness : DEFAULT_LIGHTING.brightness ?? 128;
    liveBrightness = clampNumber(next, 0, 255);
  });

  $effect(() => {
    if (frequencyKhzTouched) return;
    const hz = typeof form.frequency_hz === 'number' && Number.isFinite(form.frequency_hz) ? form.frequency_hz : DEFAULT_LIGHTING.frequency_hz;
    frequencyKhz = clampNumber(Math.round(hz / 1000), MIN_FREQ_KHZ, MAX_FREQ_KHZ);
  });

  $effect(() => {
    const count = lightingCount || DEFAULT_LIGHTING.count;
    if (ledColors.length !== count) {
      ledColors = Array.from({ length: count }, (_, idx) => ledColors[idx] ?? '#00c8ff');
    }
    if (ledWhites.length !== count) {
      ledWhites = Array.from({ length: count }, (_, idx) => clampNumber(ledWhites[idx] ?? 0, 0, 255));
    }
    if (ledSelectionBrightness.length !== count) {
      ledSelectionBrightness = Array.from({ length: count }, (_, idx) => clampNumber(ledSelectionBrightness[idx] ?? 255, 0, 255));
    }
    const safeSelection = Array.from(
      new Set(selectedLedIndices.map((index) => clampNumber(index, 0, Math.max(0, count - 1))).filter((index) => index >= 0 && index < count))
    );
    const nextSelection = safeSelection;
    const changed =
      nextSelection.length !== selectedLedIndices.length || nextSelection.some((value, idx) => value !== selectedLedIndices[idx]);
    if (changed) {
      selectedLedIndices = nextSelection;
      syncEditorFromSelection();
    }
  });

  function defaultAnimationRefForEvent(eventKey: string): string {
    const normalizedEvent = eventKey.trim().toLowerCase();
    if (!normalizedEvent.length) return '';
    const mapping = form.default_animations ?? {};
    for (const [key, value] of Object.entries(mapping)) {
      if (key.trim().toLowerCase() === normalizedEvent && typeof value === 'string') {
        return value.trim();
      }
    }
    return '';
  }

  function setDefaultAnimationRefForEvent(eventKey: string, animationName: string): void {
    const normalizedEvent = eventKey.trim().toLowerCase();
    if (!normalizedEvent.length) return;
    const next = { ...(form.default_animations ?? {}) } as Record<string, string>;
    const trimmedName = animationName.trim();
    if (!trimmedName.length) {
      delete next[normalizedEvent];
    } else {
      next[normalizedEvent] = trimmedName;
    }
    form = {
      ...form,
      default_animations: next
    };
  }

  $effect(() => {
    if (sequenceBusy || timelineKeyframes.length === 0) return;
    const sampled = sampleTimelineFrameAt(timelineCursorMs);
    if (sampled) {
      untrack(() => {
        applyEditorFrameTransient(sampled);
      });
    }
  });

  function validate(): string | null {
    if (!Number.isFinite(form.count) || form.count <= 0) {
      return 'LED count must be at least 1.';
    }
    if (!Number.isFinite(form.gpio) || form.gpio < 0) {
      return 'GPIO pin must be a valid BCM pin.';
    }
    const frequency = coerceInt(form.frequency_hz);
    const frequencyKhzValue = Math.round(frequency / 1000);
    if (frequencyKhzValue < MIN_FREQ_KHZ || frequencyKhzValue > MAX_FREQ_KHZ) {
      return `PWM/bitstream frequency must be between ${MIN_FREQ_KHZ} and ${MAX_FREQ_KHZ.toLocaleString()} kHz.`;
    }
    form.frequency_hz = frequency;
    if (typeof form.brightness === 'number') {
      if (!Number.isFinite(form.brightness) || form.brightness < 0 || form.brightness > 255) {
        return 'Brightness must be between 0 and 255.';
      }
    }
    const order = form.color_order.trim();
    if (!order.length) return 'Color order is required.';
    const protocol = form.protocol.trim();
    if (!protocol.length) return 'Protocol label is required.';
    if (form.label != null && !form.label.trim().length) {
      return 'Label cannot be empty when provided.';
    }
    return null;
  }

  function makeId(): string {
    return `keyframe-${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
  }

  function colorPayloadToHex(color: LightingColorPayload | null | undefined): string {
    if (!color) return '#000000';
    return rgbToHex({
      r: clampNumber(color.r ?? 0, 0, 255),
      g: clampNumber(color.g ?? 0, 0, 255),
      b: clampNumber(color.b ?? 0, 0, 255)
    });
  }

  function applyEditorFrame(frame: LightingColorPayload[]): void {
    const resolvedCount = Math.max(1, frame.length || lightingCount || DEFAULT_LIGHTING.count);
    const nextColors = Array.from({ length: resolvedCount }, (_, idx) => colorPayloadToHex(frame[idx]));
    const nextWhites = Array.from({ length: resolvedCount }, (_, idx) => clampNumber(frame[idx]?.w ?? 0, 0, 255));
    const nextBrightnesses = Array.from({ length: resolvedCount }, () => 255);
    commitEditorChange(() => {
      lightingCount = resolvedCount;
      form.count = resolvedCount;
      ledColors = nextColors;
      ledWhites = nextWhites;
      ledSelectionBrightness = nextBrightnesses;
      selectedLedIndices = [0];
      syncEditorFromSelection();
    });
  }

  function applyEditorFrameTransient(frame: LightingColorPayload[]): void {
    const resolvedCount = Math.max(1, frame.length || lightingCount || DEFAULT_LIGHTING.count);
    lightingCount = resolvedCount;
    form.count = resolvedCount;
    ledColors = Array.from({ length: resolvedCount }, (_, idx) => colorPayloadToHex(frame[idx]));
    ledWhites = Array.from({ length: resolvedCount }, (_, idx) => clampNumber(frame[idx]?.w ?? 0, 0, 255));
    ledSelectionBrightness = Array.from({ length: resolvedCount }, () => 255);
    syncEditorFromSelection();
  }

  function timelinePayloadToKeyframes(timeline: LightingTimelinePayload, fallbackCount: number): TimelineKeyframe[] {
    const keyframes = timeline.keyframes ?? [];
    const inferredCount = Math.max(fallbackCount, ...keyframes.map((frame) => frame.frame?.length ?? 0), 1);
    return normalizeTimelineKeyframes(
      keyframes.map((frame, idx) => ({
        id: makeId(),
        name: `Keyframe ${idx + 1}`,
        time_ms: Math.max(0, Math.trunc(Number(frame.time_ms) || 0)),
        colors: Array.from({ length: inferredCount }, (_, ledIdx) => colorPayloadToHex(frame.frame?.[ledIdx])),
        whites: Array.from({ length: inferredCount }, (_, ledIdx) => clampNumber(frame.frame?.[ledIdx]?.w ?? 0, 0, 255)),
        brightnesses: Array.from({ length: inferredCount }, () => 255),
        easing: (frame.easing ?? 'linear') as LightingTimelineEasing
      }))
    );
  }

  function framesPayloadToKeyframes(frames: LightingFramePayload[], fallbackCount: number): TimelineKeyframe[] {
    const inferredCount = Math.max(fallbackCount, ...frames.map((frame) => frame.frame?.length ?? 0), 1);
    let cursor = 0;
    const keyframes = frames.map((frame, idx) => {
      const keyframe: TimelineKeyframe = {
        id: makeId(),
        name: `Keyframe ${idx + 1}`,
        time_ms: cursor,
        colors: Array.from({ length: inferredCount }, (_, ledIdx) => colorPayloadToHex(frame.frame?.[ledIdx])),
        whites: Array.from({ length: inferredCount }, (_, ledIdx) => clampNumber(frame.frame?.[ledIdx]?.w ?? 0, 0, 255)),
        brightnesses: Array.from({ length: inferredCount }, () => 255),
        easing: 'linear'
      };
      cursor += Math.max(20, Math.trunc(Number(frame.duration_ms) || timelineSampleMs || 50));
      return keyframe;
    });
    return normalizeTimelineKeyframes(keyframes);
  }

  function isLitColor(color: LightingColorPayload | null | undefined): boolean {
    if (!color) return false;
    return (Number(color.r) || 0) + (Number(color.g) || 0) + (Number(color.b) || 0) + (Number(color.w) || 0) > 0;
  }

  function normalizeFramesForEditor(frames: LightingFramePayload[], fallbackCount: number): LightingFramePayload[] {
    const inferredCount = Math.max(fallbackCount, ...frames.map((frame) => frame.frame?.length ?? 0), 1);
    const padded = frames.map((frame) => ({
      duration_ms: Math.max(20, Math.trunc(Number(frame.duration_ms) || 120)),
      frame: Array.from({ length: inferredCount }, (_, idx) => frame.frame?.[idx] ?? { r: 0, g: 0, b: 0, w: 0 })
    }));
    if (padded.length < 2) return padded;

    const litCounts = padded.map((item) => item.frame.reduce((sum, color) => sum + (isLitColor(color) ? 1 : 0), 0));
    const monotonicProgress = litCounts.every((value, idx) => idx === 0 || value >= litCounts[idx - 1]) && litCounts.some((value, idx) => idx > 0 && value > litCounts[idx - 1]);
    const finalLit = litCounts[litCounts.length - 1] ?? 0;
    if (!monotonicProgress || finalLit >= inferredCount) {
      return padded;
    }

    const lastFrame = padded[padded.length - 1];
    const highlight =
      lastFrame.frame.find((color) => isLitColor(color)) ??
      ({ r: 255, g: 255, b: 255, w: 0 } as LightingColorPayload);
    const out = [...padded];
    let workingFrame = [...lastFrame.frame];
    for (let idx = finalLit; idx < inferredCount; idx += 1) {
      workingFrame = [...workingFrame];
      workingFrame[idx] = { ...highlight };
      out.push({
        duration_ms: lastFrame.duration_ms,
        frame: [...workingFrame]
      });
    }
    return out;
  }

  function applyTimelineEasing(easing: LightingTimelineEasing | undefined, t: number): number {
    const value = Math.max(0, Math.min(1, t));
    switch (easing ?? 'linear') {
      case 'step':
        return value >= 1 ? 1 : 0;
      case 'ease_in':
        return value * value;
      case 'ease_out':
        return 1 - (1 - value) * (1 - value);
      case 'ease_in_out':
        return value < 0.5 ? 2 * value * value : 1 - ((-2 * value + 2) ** 2) / 2;
      default:
        return value;
    }
  }

  function lerpChannel(a: number, b: number, t: number): number {
    return clampNumber(Math.round(a + (b - a) * t), 0, 255);
  }

  function frameFromKeyframe(keyframe: TimelineKeyframe): LightingColorPayload[] {
    return buildFramePayloadFrom(
      keyframe.colors.map((hex, idx) => scaleHex(hex, clampNumber(keyframe.brightnesses?.[idx] ?? 255, 0, 255) / 255)),
      keyframe.whites
    );
  }

  function interpolateFrames(a: LightingColorPayload[], b: LightingColorPayload[], t: number): LightingColorPayload[] {
    const count = Math.max(a.length, b.length);
    return Array.from({ length: count }, (_, idx) => {
      const left = a[idx] ?? { r: 0, g: 0, b: 0, w: 0 };
      const right = b[idx] ?? { r: 0, g: 0, b: 0, w: 0 };
      return {
        r: lerpChannel(left.r ?? 0, right.r ?? 0, t),
        g: lerpChannel(left.g ?? 0, right.g ?? 0, t),
        b: lerpChannel(left.b ?? 0, right.b ?? 0, t),
        w: lerpChannel(left.w ?? 0, right.w ?? 0, t)
      };
    });
  }

  function sampleTimelineFrameAt(timeMs: number): LightingColorPayload[] | null {
    if (timelineKeyframes.length === 0) return null;
    const sorted = normalizeTimelineKeyframes(timelineKeyframes);
    if (sorted.length === 1) {
      return frameFromKeyframe(sorted[0]);
    }
    const cursor = Math.max(0, Math.trunc(timeMs || 0));
    if (cursor <= sorted[0].time_ms) {
      return frameFromKeyframe(sorted[0]);
    }
    for (let idx = 0; idx < sorted.length - 1; idx += 1) {
      const start = sorted[idx];
      const end = sorted[idx + 1];
      if (cursor > end.time_ms) continue;
      const span = Math.max(1, end.time_ms - start.time_ms);
      const rawT = Math.max(0, Math.min(1, (cursor - start.time_ms) / span));
      const easedT = applyTimelineEasing(start.easing, rawT);
      return interpolateFrames(frameFromKeyframe(start), frameFromKeyframe(end), easedT);
    }
    return frameFromKeyframe(sorted[sorted.length - 1]);
  }

  function setTimelineCursorMs(value: number, syncEditor = true): void {
    timelineCursorMs = Math.max(0, Math.trunc(value || 0));
    if (!syncEditor || sequenceBusy) return;
    const sampled = sampleTimelineFrameAt(timelineCursorMs);
    if (sampled) {
      applyEditorFrameTransient(sampled);
    }
  }

  function animationPayloadToKeyframes(
    animation: NonNullable<SavedLightingAnimation['animation']>,
    ledCount: number,
    baseBrightness: number
  ): { keyframes: TimelineKeyframe[]; sampleMs: number; durationMs: number } {
    const count = Math.max(1, Math.trunc(ledCount || 1));
    const offColors = Array.from({ length: count }, () => '#000000');
    const offWhites = Array.from({ length: count }, () => 0);
    if (animation.kind === 'off') {
      return {
        keyframes: [
          {
            id: makeId(),
            name: 'Keyframe 1',
            time_ms: 0,
            colors: offColors,
            whites: offWhites,
            brightnesses: Array.from({ length: count }, () => 255),
            easing: 'step'
          }
        ],
        sampleMs: 120,
        durationMs: 600
      };
    }

    const animationColor = colorPayloadToHex((animation as { color?: LightingColorPayload }).color);
    const animationWhite = clampNumber((animation as { color?: LightingColorPayload }).color?.w ?? 0, 0, 255);
    const speedHz = Number.isFinite((animation as { speed_hz?: number }).speed_hz) ? Math.max(0.1, Number((animation as { speed_hz?: number }).speed_hz)) : 8;
    const low = clampNumber((animation as { low?: number }).low ?? 8, 0, 255);
    const high = clampNumber((animation as { high?: number }).high ?? 255, 0, 255);
    const periodMs = Math.max(120, Math.trunc((animation as { period_ms?: number }).period_ms ?? 900));

    if (animation.kind === 'pulse') {
      const colors = Array.from({ length: count }, () => animationColor);
      const whites = Array.from({ length: count }, () => animationWhite);
      return {
        keyframes: normalizeTimelineKeyframes([
          {
            id: makeId(),
            name: 'Keyframe 1',
            time_ms: 0,
            colors,
            whites,
            brightnesses: Array.from({ length: count }, () => low),
            easing: 'ease_in_out'
          },
          {
            id: makeId(),
            name: 'Keyframe 2',
            time_ms: Math.trunc(periodMs / 2),
            colors,
            whites,
            brightnesses: Array.from({ length: count }, () => high),
            easing: 'ease_in_out'
          },
          {
            id: makeId(),
            name: 'Keyframe 3',
            time_ms: periodMs,
            colors,
            whites,
            brightnesses: Array.from({ length: count }, () => low),
            easing: 'ease_in_out'
          }
        ]),
        sampleMs: Math.max(80, Math.trunc(periodMs / 8)),
        durationMs: periodMs
      };
    }

    const spanSteps = Math.max(1, Math.ceil((Math.max(low, high) - Math.min(low, high)) / 4));
    const pulseCycleSteps = Math.max(2, spanSteps * 2);
    const stepDelayMs = Math.max(1, Math.floor(periodMs / 32));
    const pulseCycleMs = pulseCycleSteps * stepDelayMs;
    const rainbowCycleMs = Math.max(600, Math.round(90_000 / speedHz));
    const chaseCycleMs = Math.max(300, Math.round((count * 1000) / speedHz));
    const hueStep = Math.max(1, Math.round(speedHz * 4 * (stepDelayMs / 1000)));
    const hueCycleMs = Math.max(stepDelayMs, Math.ceil(360 / hueStep) * stepDelayMs);

    const durationMs = (() => {
      switch (animation.kind) {
        case 'chase':
          return Math.min(20_000, chaseCycleMs);
        case 'rainbow':
          return Math.min(20_000, rainbowCycleMs);
        case 'breathing_rainbow':
          return Math.min(20_000, Math.max(periodMs, pulseCycleMs, hueCycleMs));
        default:
          return 1_200;
      }
    })();

    const sampleMs = (() => {
      switch (animation.kind) {
        case 'chase':
          return clampNumber(Math.round((1000 / speedHz) / 2), 20, 240);
        case 'breathing_rainbow':
          return clampNumber(Math.round(stepDelayMs), 20, 240);
        case 'rainbow':
          return clampNumber(Math.round((1000 / speedHz) / 2), 20, 240);
        default:
          return 80;
      }
    })();

    const times = Array.from({ length: Math.max(2, Math.floor(durationMs / sampleMs) + 1) }, (_, idx) => idx * sampleMs);
    if (times[times.length - 1] !== durationMs) {
      times.push(durationMs);
    }

    const easing: LightingTimelineEasing = animation.kind === 'chase' ? 'step' : animation.kind === 'breathing_rainbow' ? 'ease_in_out' : 'linear';
    const keyframes: TimelineKeyframe[] = [];
    let previousFingerprint: string | null = null;
    for (const timeMs of times) {
      const preview = buildPreviewFrame({
        ledColors: offColors,
        ledWhites: offWhites,
        liveBrightness: clampNumber(baseBrightness, 0, 255),
        showUiPreview: true,
        animationKind: animation.kind,
        animationSpeedHz: speedHz,
        animationLow: low,
        animationHigh: high,
        animationPeriodMs: periodMs,
        timeline: null,
        showTimelinePreview: false,
        previewTickMs: timeMs,
        previewStartMs: 0,
        animationColor,
        animationWhite
      });
      const colors = Array.from({ length: count }, (_, idx) => preview.colors[idx] ?? '#000000');
      const whites = Array.from({ length: count }, (_, idx) => clampNumber(preview.whites[idx] ?? 0, 0, 255));
      const brightnesses = Array.from({ length: count }, () => clampNumber(preview.brightness ?? baseBrightness, 0, 255));
      const fingerprint = JSON.stringify([colors, whites, brightnesses]);
      const isBoundary = timeMs === 0 || timeMs === durationMs;
      if (!isBoundary && previousFingerprint === fingerprint) {
        continue;
      }
      previousFingerprint = fingerprint;
      keyframes.push({
        id: makeId(),
        name: `Keyframe ${keyframes.length + 1}`,
        time_ms: timeMs,
        colors,
        whites,
        brightnesses,
        easing
      });
    }

    return {
      keyframes: normalizeTimelineKeyframes(keyframes),
      sampleMs,
      durationMs
    };
  }

  function arraysEqual(a: number[] | string[], b: number[] | string[]): boolean {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i += 1) {
      if (a[i] !== b[i]) return false;
    }
    return true;
  }

  function snapshotEditorState(): EditorStateSnapshot {
    return {
      ledColors: [...ledColors],
      ledWhites: [...ledWhites],
      ledSelectionBrightness: [...ledSelectionBrightness],
      selectedLedIndices: [...selectedLedIndices],
      editorColor,
      editorBrightness
    };
  }

  function editorStatesEqual(a: EditorStateSnapshot, b: EditorStateSnapshot): boolean {
    return (
      arraysEqual(a.ledColors, b.ledColors) &&
      arraysEqual(a.ledWhites, b.ledWhites) &&
      arraysEqual(a.ledSelectionBrightness, b.ledSelectionBrightness) &&
      arraysEqual(a.selectedLedIndices, b.selectedLedIndices) &&
      a.editorColor === b.editorColor &&
      a.editorBrightness === b.editorBrightness
    );
  }

  function restoreEditorState(snapshot: EditorStateSnapshot): void {
    ledColors = [...snapshot.ledColors];
    ledWhites = [...snapshot.ledWhites];
    ledSelectionBrightness = [...snapshot.ledSelectionBrightness];
    selectedLedIndices = [...snapshot.selectedLedIndices];
    editorColor = snapshot.editorColor;
    editorBrightness = snapshot.editorBrightness;
  }

  function commitEditorChange(change: () => void): void {
    const before = snapshotEditorState();
    change();
    const after = snapshotEditorState();
    if (editorStatesEqual(before, after)) return;
    editorUndoStack = [...editorUndoStack, before].slice(-EDITOR_HISTORY_LIMIT);
    editorRedoStack = [];
  }

  function undoEditorChange(): void {
    const previous = editorUndoStack.at(-1);
    if (!previous) return;
    const current = snapshotEditorState();
    editorUndoStack = editorUndoStack.slice(0, -1);
    editorRedoStack = [...editorRedoStack, current].slice(-EDITOR_HISTORY_LIMIT);
    restoreEditorState(previous);
  }

  function redoEditorChange(): void {
    const next = editorRedoStack.at(-1);
    if (!next) return;
    const current = snapshotEditorState();
    editorRedoStack = editorRedoStack.slice(0, -1);
    editorUndoStack = [...editorUndoStack, current].slice(-EDITOR_HISTORY_LIMIT);
    restoreEditorState(next);
  }

  function isEditableElement(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    if (target.isContentEditable || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT') return true;
    if (target.tagName !== 'INPUT') return false;
    const inputType = (target as HTMLInputElement).type.toLowerCase();
    return ['text', 'search', 'url', 'tel', 'password', 'email', 'number'].includes(inputType);
  }

  function handleEditorKeydown(event: KeyboardEvent): void {
    if (isEditableElement(event.target)) return;
    if (!(event.ctrlKey || event.metaKey)) return;
    if (event.altKey) return;
    if (event.key.toLowerCase() !== 'z') return;
    event.preventDefault();
    if (event.shiftKey) {
      redoEditorChange();
      return;
    }
    undoEditorChange();
  }

  function addTimelineKeyframe(): void {
    const id = makeId();
    const name = `Keyframe ${timelineKeyframes.length + 1}`;
    const time_ms = Math.max(0, Math.trunc(Number(timelineCursorMs) || 0));
    const next = normalizeTimelineKeyframes([
      ...timelineKeyframes,
      {
        id,
        name,
        time_ms,
        colors: [...ledColors],
        whites: [...ledWhites],
        brightnesses: [...ledSelectionBrightness],
        easing: 'linear'
      }
    ]);
    timelineKeyframes = next;
    timelineDurationMs = Math.max(timelineDurationMs, time_ms + timelineSampleMs);
    selectedTimelineKeyframeId = id;
  }

  function loadTimelineKeyframe(frame: TimelineKeyframe): void {
    commitEditorChange(() => {
      ledColors = [...frame.colors];
      ledWhites = [...frame.whites];
      ledSelectionBrightness = Array.from({ length: frame.colors.length }, (_, idx) =>
        clampNumber(frame.brightnesses?.[idx] ?? 255, 0, 255)
      );
      timelineCursorMs = frame.time_ms;
      selectedTimelineKeyframeId = frame.id;
      syncEditorFromSelection();
    });
  }

  function removeTimelineKeyframe(id: string): void {
    timelineKeyframes = timelineKeyframes.filter((frame) => frame.id !== id);
    if (selectedTimelineKeyframeId === id) {
      selectedTimelineKeyframeId = timelineKeyframes[0]?.id ?? null;
    }
  }

  function duplicateTimelineKeyframe(id: string): void {
    const source = timelineKeyframes.find((frame) => frame.id === id);
    if (!source) return;
    const timeOffset = Math.max(20, Math.trunc(Number(timelineSampleMs) || 20));
    const duplicate: TimelineKeyframe = {
      ...source,
      id: makeId(),
      name: `Keyframe ${timelineKeyframes.length + 1}`,
      time_ms: Math.max(0, source.time_ms + timeOffset),
      colors: [...source.colors],
      whites: [...source.whites],
      brightnesses: [...(source.brightnesses ?? Array.from({ length: source.colors.length }, () => 255))]
    };
    timelineKeyframes = normalizeTimelineKeyframes([...timelineKeyframes, duplicate]);
    timelineDurationMs = Math.max(timelineDurationMs, duplicate.time_ms + timeOffset);
    selectedTimelineKeyframeId = duplicate.id;
    timelineCursorMs = duplicate.time_ms;
    loadTimelineKeyframe(duplicate);
  }

  function clearTimelineKeyframes(): void {
    timelineKeyframes = [];
    selectedTimelineKeyframeId = null;
    timelineCursorMs = 0;
  }

  function updateTimelineKeyframeTime(id: string, value: number): void {
    const safeValue = Math.max(0, Math.trunc(Number(value) || 0));
    timelineKeyframes = normalizeTimelineKeyframes(
      timelineKeyframes.map((frame) => (frame.id === id ? { ...frame, time_ms: safeValue } : frame))
    );
    timelineDurationMs = Math.max(
      timelineDurationMs,
      ...timelineKeyframes.map((frame) => (frame.id === id ? safeValue : frame.time_ms))
    );
  }

  function updateTimelineKeyframeEasing(id: string, easing: LightingTimelineEasing): void {
    timelineKeyframes = timelineKeyframes.map((frame) => (frame.id === id ? { ...frame, easing } : frame));
  }

  function buildTimelinePayloadForEditor(durationOverride: number | null = null): LightingTimelinePayload {
    const durationValue = Number(durationOverride);
    const resolvedDuration = Number.isFinite(durationValue) ? Math.max(0, Math.trunc(durationValue)) : timelineDurationMs;
    return buildTimelinePayloadFromKeyframes(timelineKeyframes, {
      durationMs: resolvedDuration,
      sampleMs: timelineSampleMs
    });
  }

  function stopTimelineSequence(): void {
    sequenceToken += 1;
    sequenceBusy = false;
    liveBusy = false;
  }

  async function playFrameSequence(
    frames: LightingFramePayload[],
    brightness: number,
    label: string,
    options?: { loop?: boolean; updateCursor?: boolean }
  ): Promise<void> {
    sequenceError = null;
    liveError = null;
    liveStatus = null;
    sequenceBusy = true;
    liveBusy = true;
    const token = sequenceToken + 1;
    sequenceToken = token;
    const loopPlayback = options?.loop === true;
    try {
      do {
        let loopCursor = 0;
        for (const frame of frames) {
          if (sequenceToken !== token) {
            break;
          }
          applyEditorFrameTransient(frame.frame ?? []);
          if (options?.updateCursor) {
            setTimelineCursorMs(loopCursor, false);
          }
          const body: Record<string, unknown> = {
            requested_by: REQUESTED_BY,
            frame: frame.frame,
            brightness
          };
          await apiFetch('/device/lighting', { method: 'POST', body: JSON.stringify(body) });
          const frameDuration = Math.max(50, frame.duration_ms || 0);
          await new Promise((resolve) => setTimeout(resolve, frameDuration));
          loopCursor += frameDuration;
        }
      } while (loopPlayback && sequenceToken === token);
      if (sequenceToken === token) {
        liveStatus = label;
      }
    } catch (err) {
      sequenceError = buildErrorMessage({ error: err, fallback: 'Unable to preview the timeline sequence.' });
    } finally {
      if (sequenceToken === token) {
        sequenceBusy = false;
        liveBusy = false;
      }
    }
  }

  async function playTimelineOnDevice(): Promise<void> {
    if (timelineKeyframes.length === 0) {
      sequenceError = 'Insert at least one keyframe to preview a timeline.';
      return;
    }
    const timeline = buildTimelinePayloadForEditor();
    const frames = compileTimelineToFrameSequence(timeline);
    await playFrameSequence(frames, clampNumber(liveBrightness), timelineLoop ? 'Playing timeline (looping)' : 'Played timeline sequence', {
      loop: timelineLoop,
      updateCursor: true
    });
  }

  function buildScaledColorsForOutput(colors: string[], brightnesses: number[]): string[] {
    return colors.map((hex, idx) => scaleHex(hex, clampNumber(brightnesses[idx] ?? 255, 0, 255) / 255));
  }

  function buildCurrentFramePayloadForOutput() {
    const scaledColors = buildScaledColorsForOutput(ledColors, ledSelectionBrightness);
    return buildFramePayloadFrom(scaledColors, ledWhites);
  }

  async function previewCurrentFrameOnDevice(): Promise<void> {
    liveBusy = true;
    liveStatus = null;
    liveError = null;
    try {
      const body: Record<string, unknown> = {
        requested_by: REQUESTED_BY,
        frame: buildCurrentFramePayloadForOutput(),
        brightness: clampNumber(liveBrightness)
      };
      await apiFetch('/device/lighting', { method: 'POST', body: JSON.stringify(body) });
      if (!previewPrimed) {
        previewPrimed = true;
        await apiFetch('/device/lighting', { method: 'POST', body: JSON.stringify(body) });
      }
      liveStatus = 'Previewed current frame';
    } catch (err) {
      liveError = buildErrorMessage({ error: err, fallback: 'Unable to preview current frame.' });
    } finally {
      liveBusy = false;
    }
  }

  async function stopDeviceOutput(): Promise<void> {
    stopTimelineSequence();
    liveBusy = true;
    liveError = null;
    liveStatus = null;
    try {
      const offFrame = Array.from({ length: Math.max(1, lightingCount || DEFAULT_LIGHTING.count) }, () => ({ r: 0, g: 0, b: 0, w: 0 }));
      await stopLightingOutput(apiFetch, offFrame, REQUESTED_BY);
      liveStatus = 'Stopped device output';
      stopLightingAnimationTicker();
      const state = await fetchLightingRuntimeState(apiFetch);
      syncFromRuntimeState(state, 'http');
    } catch (err) {
      liveError = buildErrorMessage({ error: err, fallback: 'Unable to stop lighting output.' });
    } finally {
      liveBusy = false;
    }
  }

  function templateToSavedAnimation(template: LightingAnimationTemplateDocument): SavedLightingAnimation {
    return {
      name: template.name,
      frame: template.frame ?? null,
      frames: template.frames ?? null,
      timeline: template.timeline ?? null,
      brightness: template.brightness ?? null,
      animation: template.animation ?? null,
      duration_ms: template.duration_ms ?? null
    };
  }

  function applyAnimationEntryToEditor(entry: SavedLightingAnimation): boolean {
    if (typeof entry.brightness === 'number' && Number.isFinite(entry.brightness)) {
      liveBrightness = clampNumber(entry.brightness, 0, 255);
    }

    if (entry.timeline?.keyframes?.length) {
      const keyframes = timelinePayloadToKeyframes(entry.timeline, Math.max(1, lightingCount));
      timelineKeyframes = keyframes;
      const sampleMs = Math.max(20, Math.trunc(Number(entry.timeline.sample_ms) || timelineSampleMs || 50));
      timelineSampleMs = sampleMs;
      const lastFrameTime = keyframes[keyframes.length - 1]?.time_ms ?? 0;
      timelineDurationMs = Math.max(100, Math.trunc(Number(entry.timeline.duration_ms) || 0), lastFrameTime + sampleMs);
      const firstKeyframe = keyframes[0];
      selectedTimelineKeyframeId = firstKeyframe?.id ?? null;
      timelineCursorMs = firstKeyframe?.time_ms ?? 0;
      applyEditorFrame(entry.timeline.keyframes[0]?.frame ?? []);
      return true;
    }

    if (entry.frames?.length) {
      const normalizedFrames = normalizeFramesForEditor(entry.frames, Math.max(1, lightingCount));
      const keyframes = framesPayloadToKeyframes(normalizedFrames, Math.max(1, lightingCount));
      timelineKeyframes = keyframes;
      const sampleMs = Math.max(20, Math.trunc(Number(normalizedFrames[0]?.duration_ms) || timelineSampleMs || 50));
      timelineSampleMs = sampleMs;
      const totalDuration = normalizedFrames.reduce((sum, frame) => sum + Math.max(20, Math.trunc(Number(frame.duration_ms) || sampleMs)), 0);
      const lastFrameTime = keyframes[keyframes.length - 1]?.time_ms ?? 0;
      timelineDurationMs = Math.max(100, Math.trunc(Number(entry.duration_ms) || 0), totalDuration, lastFrameTime + sampleMs);
      const firstKeyframe = keyframes[0];
      selectedTimelineKeyframeId = firstKeyframe?.id ?? null;
      timelineCursorMs = firstKeyframe?.time_ms ?? 0;
      applyEditorFrame(normalizedFrames[0]?.frame ?? []);
      return true;
    }

    if (entry.frame?.length) {
      clearTimelineKeyframes();
      applyEditorFrame(entry.frame);
      return true;
    }

    if (entry.animation) {
      const inferredCount = Math.max(1, lightingCount || DEFAULT_LIGHTING.count);
      const approximation = animationPayloadToKeyframes(entry.animation, inferredCount, clampNumber(entry.brightness ?? liveBrightness, 0, 255));
      if (approximation.keyframes.length === 0) {
        return false;
      }
      timelineKeyframes = approximation.keyframes;
      timelineSampleMs = Math.max(20, Math.trunc(approximation.sampleMs || timelineSampleMs || 50));
      const lastFrameTime = approximation.keyframes[approximation.keyframes.length - 1]?.time_ms ?? 0;
      timelineDurationMs = Math.max(100, approximation.durationMs, lastFrameTime + timelineSampleMs);
      const firstKeyframe = approximation.keyframes[0];
      if (firstKeyframe) {
        loadTimelineKeyframe(firstKeyframe);
      }
      return true;
    }

    return false;
  }

  async function loadSelectedAnimationIntoEditor(): Promise<void> {
    animationFormError = null;
    animationFormStatus = null;
    const selected = decodeLoadAnimationRef(selectedLoadAnimationRef);
    if (!selected || !selected.value.trim().length) {
      animationFormError = 'Select an animation or template to load.';
      return;
    }

    animationLoadBusy = true;
    try {
      if (selected.kind === 'saved') {
        const entry = visibleSavedAnimations.find((item) => item.name === selected.value);
        if (!entry) {
          animationFormError = 'Selected animation no longer exists.';
          return;
        }
        animationName = entry.name;
        if (applyAnimationEntryToEditor(entry)) {
          animationFormStatus = `Loaded ${entry.name}`;
        } else {
          animationFormError = 'This saved animation is not timeline/frame editable yet.';
        }
      } else {
        const selectedKey = normalizedAnimationKey(selected.value);
        const matchedTemplate = lightingTemplates.find(
          (entry) => entry.template_id === selected.value || normalizedAnimationKey(entry.name) === selectedKey
        );
        if (matchedTemplate) {
          const template = await fetchLightingTemplate(apiFetch, matchedTemplate.template_id);
          const entry = templateToSavedAnimation(template);
          animationName = entry.name;
          if (applyAnimationEntryToEditor(entry)) {
            animationFormStatus = `Loaded template ${template.name}`;
          } else {
            animationFormStatus = `Loaded template ${template.name} (effect preset, not timeline editable).`;
          }
        } else {
          const fallbackTemplate = fallbackTemplateAnimations.find((entry) => normalizedAnimationKey(entry.name) === selectedKey);
          if (!fallbackTemplate) {
            animationFormError = 'Selected template no longer exists.';
            return;
          }
          animationName = fallbackTemplate.name;
          if (applyAnimationEntryToEditor(fallbackTemplate)) {
            animationFormStatus = `Loaded template ${fallbackTemplate.name}`;
          } else {
            animationFormStatus = `Loaded template ${fallbackTemplate.name} (effect preset, not timeline editable).`;
          }
        }
      }
    } catch (err) {
      animationFormError = buildErrorMessage({ error: err, fallback: 'Unable to load the selected item.' });
    } finally {
      animationLoadBusy = false;
    }
  }

  type LightingAnimationImportBundle = {
    version?: number;
    exported_at_ms?: number;
    animations?: SavedLightingAnimation[];
  };

  function currentAnimationExportName(): string {
    const base = animationName.trim() || 'lighting-animations';
    const safe = base
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '');
    const stamp = new Date()
      .toISOString()
      .replace(/[-:]/g, '')
      .replace(/\..+/, '')
      .replace('T', '-');
    return `${safe || 'lighting-animations'}-${stamp}.json`;
  }

  function buildAnimationImportBundle(): LightingAnimationImportBundle {
    return {
      version: 1,
      exported_at_ms: Date.now(),
      animations: savedAnimations.map((entry) => ({
        name: entry.name,
        frame: entry.frame ?? null,
        frames: entry.frames ?? null,
        timeline: entry.timeline ?? null,
        brightness: entry.brightness ?? null,
        animation: entry.animation ?? null,
        duration_ms: entry.duration_ms ?? null
      }))
    };
  }

  function buildSaveBodyFromImportedEntry(entry: SavedLightingAnimation, fallbackName: string): Record<string, unknown> | null {
    const name = entry.name?.trim() || fallbackName;
    const body: Record<string, unknown> = {
      name,
      requested_by: REQUESTED_BY
    };
    if (typeof entry.brightness === 'number' && Number.isFinite(entry.brightness)) {
      body.brightness = clampNumber(entry.brightness, 0, 255);
    }
    if (entry.timeline?.keyframes?.length) {
      body.timeline = entry.timeline;
      if (typeof entry.duration_ms === 'number' && Number.isFinite(entry.duration_ms)) {
        body.duration_ms = Math.max(0, Math.trunc(entry.duration_ms));
      }
      return body;
    }
    if (entry.frames?.length) {
      body.frames = entry.frames;
      if (typeof entry.duration_ms === 'number' && Number.isFinite(entry.duration_ms)) {
        body.duration_ms = Math.max(0, Math.trunc(entry.duration_ms));
      }
      return body;
    }
    if (entry.frame?.length) {
      body.frame = entry.frame;
      if (typeof entry.duration_ms === 'number' && Number.isFinite(entry.duration_ms)) {
        body.duration_ms = Math.max(0, Math.trunc(entry.duration_ms));
      }
      return body;
    }
    if (entry.animation) {
      body.animation = entry.animation;
      if (typeof entry.duration_ms === 'number' && Number.isFinite(entry.duration_ms)) {
        body.duration_ms = Math.max(0, Math.trunc(entry.duration_ms));
      }
      return body;
    }
    return null;
  }

  function normalizeImportedEntries(payload: unknown): SavedLightingAnimation[] {
    if (payload == null) return [];
    if (Array.isArray(payload)) return payload as SavedLightingAnimation[];
    if (typeof payload !== 'object') return [];
    const record = payload as Record<string, unknown>;
    if (Array.isArray(record.animations)) {
      return record.animations as SavedLightingAnimation[];
    }
    if (
      typeof record.name === 'string' &&
      (Array.isArray(record.frame) || Array.isArray(record.frames) || typeof record.animation === 'object' || typeof record.timeline === 'object')
    ) {
      return [record as SavedLightingAnimation];
    }
    return [];
  }

  async function importAnimationEntries(entries: SavedLightingAnimation[], sourceLabel: string): Promise<void> {
    animationFormError = null;
    animationFormStatus = null;
    if (!entries.length) {
      animationFormError = 'No animations found in uploaded JSON.';
      return;
    }

    animationImportBusy = true;
    try {
      let imported = 0;
      for (let idx = 0; idx < entries.length; idx += 1) {
        const fallbackName = `Imported Animation ${idx + 1}`;
        const body = buildSaveBodyFromImportedEntry(entries[idx], fallbackName);
        if (!body) continue;
        await saveLightingAnimation(apiFetch, body);
        imported += 1;
      }
      if (imported === 0) {
        animationFormError = 'Imported file did not contain usable animation entries.';
        return;
      }
      await loadSavedAnimations();
      animationFormStatus = `Imported ${imported} animation${imported === 1 ? '' : 's'} from ${sourceLabel}`;
    } catch (err) {
      animationFormError = buildErrorMessage({ error: err, fallback: 'Unable to import animation package.' });
    } finally {
      animationImportBusy = false;
    }
  }

  function exportAnimationsPackage(): void {
    animationFormError = null;
    animationFormStatus = null;
    const bundle = buildAnimationImportBundle();
    const fileName = currentAnimationExportName();
    const blob = new Blob([JSON.stringify(bundle, null, 2)], { type: 'application/json' });
    const href = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = href;
    link.download = fileName;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(href);
    animationFormStatus = `Exported ${fileName}`;
  }

  function triggerAnimationUploadPicker(): void {
    animationUploadInput?.click();
  }

  async function handleAnimationUploadInput(event: Event): Promise<void> {
    const target = event.target as HTMLInputElement;
    const file = target.files?.[0] ?? null;
    target.value = '';
    if (!file) return;
    try {
      const text = await file.text();
      const parsed = JSON.parse(text) as unknown;
      const entries = normalizeImportedEntries(parsed);
      await importAnimationEntries(entries, file.name);
    } catch (err) {
      animationFormError = buildErrorMessage({ error: err, fallback: 'Unable to parse uploaded animation JSON.' });
    }
  }

  async function saveCurrentAnimation(): Promise<void> {
    animationFormError = null;
    animationFormStatus = null;
    const name = animationName.trim();
    if (!name.length) {
      animationFormError = 'Animation name is required.';
      return;
    }

    const body: Record<string, unknown> = {
      name,
      requested_by: REQUESTED_BY,
      brightness: clampNumber(liveBrightness, 0, 255)
    };

    if (timelineKeyframes.length > 0) {
      body.timeline = buildTimelinePayloadForEditor();
      body.duration_ms = Math.max(0, Math.trunc(Number(timelineDurationMs) || 0));
    } else {
      body.frame = buildCurrentFramePayloadForOutput();
    }

    animationSaveBusy = true;
    try {
      await saveLightingAnimation(apiFetch, body);
      animationFormStatus = `Saved ${name}`;
      await loadSavedAnimations();
      selectedLoadAnimationRef = encodeLoadAnimationRef('saved', name);
      syncLoadAnimationRef(selectedLoadAnimationRef);
    } catch (err) {
      animationFormError = buildErrorMessage({ error: err, fallback: 'Unable to save animation.' });
    } finally {
      animationSaveBusy = false;
    }
  }

  async function ensureDefaultAnimationEntries(defaultAnimations: Record<string, string>): Promise<void> {
    const requiredKeys = new Set<string>();
    for (const animationName of Object.values(defaultAnimations)) {
      const key = normalizedAnimationKey(animationName);
      if (key.length > 0) {
        requiredKeys.add(key);
      }
    }
    if (requiredKeys.size === 0) {
      return;
    }

    const savedKeys = new Set<string>();
    for (const entry of savedAnimations) {
      const key = normalizedAnimationKey(entry.name);
      if (key.length > 0) {
        savedKeys.add(key);
      }
    }

    const templateByNameKey = new Map<string, LightingAnimationTemplateSummary>();
    for (const template of lightingTemplates) {
      const key = normalizedAnimationKey(template.name);
      if (key.length > 0 && !templateByNameKey.has(key)) {
        templateByNameKey.set(key, template);
      }
    }

    let importedTemplate = false;
    for (const key of requiredKeys) {
      if (savedKeys.has(key)) {
        continue;
      }
      const template = templateByNameKey.get(key);
      if (!template) {
        continue;
      }
      const templateDoc = await fetchLightingTemplate(apiFetch, template.template_id);
      const saveBody = buildSaveBodyFromImportedEntry(templateToSavedAnimation(templateDoc), templateDoc.name);
      if (!saveBody) {
        continue;
      }
      await saveLightingAnimation(apiFetch, saveBody);
      savedKeys.add(key);
      importedTemplate = true;
    }

    if (importedTemplate) {
      await loadSavedAnimations();
    }
  }

  function coerceInt(value: number | string): number {
    const num = typeof value === 'string' ? Number(value) : value;
    return Number.isFinite(num) ? Math.trunc(num) : 0;
  }

  function syncEditorFromSelection(): void {
    const selected = selectedLedIndices.filter((idx) => idx >= 0 && idx < ledColors.length);
    if (!selected.length) {
      editorColor = ledColors[0] ?? '#00c8ff';
      editorBrightness = clampNumber(ledSelectionBrightness[0] ?? 255, 0, 255);
      return;
    }

    const primary = selected[0];
    editorColor = ledColors[primary] ?? '#00c8ff';
    const averageBrightness = selected.reduce((sum, idx) => sum + clampNumber(ledSelectionBrightness[idx] ?? 255, 0, 255), 0) / selected.length;
    editorBrightness = clampNumber(Math.round(averageBrightness), 0, 255);
  }

  function toggleLedSelection(index: number): void {
    commitEditorChange(() => {
      const next = clampNumber(index, 0, Math.max(0, ledColors.length - 1));
      const exists = selectedLedIndices.includes(next);
      const updated = exists ? selectedLedIndices.filter((item) => item !== next) : [...selectedLedIndices, next];
      selectedLedIndices = Array.from(new Set(updated)).sort((a, b) => a - b);
      syncEditorFromSelection();
    });
  }

  function selectAllLeds(): void {
    commitEditorChange(() => {
      selectedLedIndices = Array.from({ length: ledColors.length }, (_, idx) => idx);
      syncEditorFromSelection();
    });
  }

  function clearLedSelection(): void {
    commitEditorChange(() => {
      selectedLedIndices = [];
      syncEditorFromSelection();
    });
  }

  function handleEditorColorChange(value: string): void {
    commitEditorChange(() => {
      editorColor = value;
      if (selectedLedIndices.length === 0) return;
      const selectedSet = new Set(selectedLedIndices.map((idx) => clampNumber(idx, 0, Math.max(0, ledColors.length - 1))));
      ledColors = ledColors.map((current, i) => (selectedSet.has(i) ? value : current));
    });
  }

  function handleEditorBrightnessChange(value: number): void {
    commitEditorChange(() => {
      const next = clampNumber(value, 0, 255);
      editorBrightness = next;
      if (selectedLedIndices.length === 0) return;
      const selectedSet = new Set(selectedLedIndices.map((idx) => clampNumber(idx, 0, Math.max(0, ledSelectionBrightness.length - 1))));
      ledSelectionBrightness = ledSelectionBrightness.map((current, i) => (selectedSet.has(i) ? next : current));
    });
  }

  async function saveLighting(): Promise<void> {
    status = null;
    error = null;
    const validation = validate();
    if (validation) {
      error = validation;
      return;
    }

    busy = true;
    const brightnessValue = typeof form.brightness === 'number' ? form.brightness : null;
    const defaultAnimations = Object.fromEntries(
      Object.entries(form.default_animations ?? {})
        .map(([eventKey, animationName]) => [eventKey.trim().toLowerCase(), String(animationName ?? '').trim()])
        .filter(([eventKey, animationName]) => eventKey.length > 0 && animationName.length > 0)
    );

    const payload = {
      requested_by: REQUESTED_BY,
      lighting: {
        enabled: form.enabled,
        gpio: coerceInt(form.gpio),
        count: coerceInt(form.count),
        use_pwm: form.use_pwm,
        color_order: form.color_order.trim().toLowerCase(),
        frequency_hz: coerceInt(form.frequency_hz),
        brightness: brightnessValue == null ? null : Math.min(255, Math.max(0, coerceInt(brightnessValue))),
        label: form.label?.trim() ? form.label.trim() : null,
        protocol: form.protocol.trim() || DEFAULT_LIGHTING.protocol,
        default_animations: defaultAnimations
      }
    };

    try {
      await ensureDefaultAnimationEntries(defaultAnimations);
      await saveLightingConfig(deviceSettingsStore, payload);
      status = `Saved at ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
      onRefresh();
      toaster.success({ title: 'Lighting saved', description: 'LED configuration updated.' });
    } catch (err) {
      reportError({
        title: 'Save failed',
        error: err,
        fallback: 'Unable to save lighting settings.',
        inline: (message) => {
          error = message;
        }
      });
    } finally {
      busy = false;
    }
  }

  async function deleteSavedAnimation(name: string, source: 'panel' | 'toolbar' = 'panel'): Promise<void> {
    savedBusy = true;
    try {
      await deleteLightingAnimation(apiFetch, name);
      await loadSavedAnimations();
      if (source === 'toolbar') {
        animationFormStatus = `Deleted ${name}`;
        if (animationName.trim().toLowerCase() === name.trim().toLowerCase()) {
          animationName = '';
        }
      }
    } catch (err) {
      const message = buildErrorMessage({ error: err, fallback: 'Unable to delete animation.' });
      if (source === 'toolbar') {
        animationFormError = message;
      } else {
        liveError = message;
      }
    } finally {
      savedBusy = false;
    }
  }

  async function deleteSelectedAnimation(): Promise<void> {
    animationFormError = null;
    animationFormStatus = null;
    const selected = decodeLoadAnimationRef(selectedLoadAnimationRef);
    if (!selected || selected.kind !== 'saved' || isTemplateNamedAnimation(selected.value)) {
      animationFormError = 'Select a saved animation to delete.';
      return;
    }
    const name = selected.value.trim();
    if (!name.length) {
      animationFormError = 'Select a saved animation to delete.';
      return;
    }
    await deleteSavedAnimation(name, 'toolbar');
  }

  async function resetLightingConfig(): Promise<void> {
    settingsBusy = true;
    settingsError = null;
    try {
      const payload = await resetLightingConfigApi(apiFetch);
      form = normalizeLighting(payload, DEFAULT_LIGHTING);
      lightingCount = form.count || DEFAULT_LIGHTING.count;
      frequencyKhzTouched = false;
      await deviceSettingsStore.load({ force: true });
    } catch (err) {
      settingsError = buildErrorMessage({ error: err, fallback: 'Unable to reset lighting settings.' });
    } finally {
      settingsBusy = false;
    }
  }

  function handleClose(): void {
    stopTimelineSequence();
    dispatch('close');
    onClose();
  }

  const handleFrequencyInput = (value: number) => {
    frequencyKhzTouched = true;
    const next = clampNumber(value, MIN_FREQ_KHZ, MAX_FREQ_KHZ);
    frequencyKhz = next;
    form.frequency_hz = Math.round(next * 1000);
  };
</script>

<SensorModalShell {peripheral} onClose={handleClose} layout="stacked" maxWidthClass="max-w-[82rem]">
  {#snippet viewer()}
    <div class="space-y-4">
      <div class="flex items-center justify-between px-1">
        <p class="text-xs text-surface-400">Timeline preset editor</p>
        <button
          class="btn btn-xs preset-tonal !px-2.5"
          type="button"
          onclick={() => (showAdvanced = true)}
          aria-label="Advanced settings"
          title="Advanced settings"
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            class="h-4 w-4"
            aria-hidden="true"
          >
            <circle cx="12" cy="12" r="3.1"></circle>
            <path d="M12 2.8v2.4M12 18.8v2.4M5.5 5.5l1.7 1.7M16.8 16.8l1.7 1.7M2.8 12h2.4M18.8 12h2.4M5.5 18.5l1.7-1.7M16.8 7.2l1.7-1.7"></path>
          </svg>
        </button>
      </div>

      <div class="rounded-xl bg-surface-950/60 p-3">
        <div class="grid gap-2 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] md:items-end">
          <label class="space-y-1 text-xs text-surface-300">
            <span class="text-micro uppercase tracking-[0.18em] text-surface-500">Load existing</span>
            <select
              class="input w-full"
              value={selectedLoadAnimationRef}
              disabled={savedBusy || templatesBusy || animationLoadBusy || animationSaveBusy}
              onchange={(event) => (selectedLoadAnimationRef = (event.target as HTMLSelectElement).value)}
            >
              {#if visibleSavedAnimations.length === 0 && lightingTemplates.length === 0 && fallbackTemplateAnimations.length === 0}
                <option value="">No saved animations or templates</option>
              {:else}
                {#if visibleSavedAnimations.length > 0}
                  <optgroup label="Saved Animations">
                    {#each visibleSavedAnimations as entry (entry.name)}
                      <option value={encodeLoadAnimationRef('saved', entry.name)}>{entry.name}</option>
                    {/each}
                  </optgroup>
                {/if}
                {#if lightingTemplates.length > 0}
                  <optgroup label="Templates">
                    {#each lightingTemplates as template (template.template_id)}
                      <option value={encodeLoadAnimationRef('template', template.template_id)}>{template.name}</option>
                    {/each}
                    {#each fallbackTemplateAnimations as entry (entry.name)}
                      <option value={encodeLoadAnimationRef('template', entry.name)}>{entry.name}</option>
                    {/each}
                  </optgroup>
                {:else if fallbackTemplateAnimations.length > 0}
                  <optgroup label="Templates">
                    {#each fallbackTemplateAnimations as entry (entry.name)}
                      <option value={encodeLoadAnimationRef('template', entry.name)}>{entry.name}</option>
                    {/each}
                  </optgroup>
                {/if}
              {/if}
            </select>
          </label>

          <label class="space-y-1 text-xs text-surface-300">
            <span class="text-micro uppercase tracking-[0.18em] text-surface-500">Name</span>
            <input
              class="input w-full"
              type="text"
              placeholder="Animation name"
              value={animationName}
              disabled={animationSaveBusy}
              oninput={(event) => (animationName = (event.target as HTMLInputElement).value)}
            />
          </label>

          <div class="flex flex-wrap items-center gap-2 md:justify-end">
            <button
              class="btn btn-2xs preset-tonal"
              type="button"
              disabled={animationLoadBusy || !selectedLoadAnimationRef}
              onclick={() => void loadSelectedAnimationIntoEditor()}
            >
              {animationLoadBusy ? 'Loading…' : 'Load'}
            </button>
            <button
              class="btn btn-2xs preset-filled-primary-500"
              type="button"
              disabled={animationSaveBusy}
              onclick={() => void saveCurrentAnimation()}
            >
              {animationSaveBusy ? 'Saving…' : 'Save'}
            </button>
            <button
              class="btn btn-2xs preset-tonal"
              type="button"
              disabled={savedBusy || animationLoadBusy || animationSaveBusy || !selectedRefIsSavedEntry()}
              onclick={() => void deleteSelectedAnimation()}
            >
              Delete
            </button>
            <button class="btn btn-2xs preset-tonal" type="button" disabled={animationImportBusy} onclick={() => triggerAnimationUploadPicker()}>
              {animationImportBusy ? 'Importing…' : 'Import'}
            </button>
            <button class="btn btn-2xs preset-tonal" type="button" onclick={() => exportAnimationsPackage()}>
              Export
            </button>
          </div>
        </div>

        <input bind:this={animationUploadInput} class="hidden" type="file" accept=".json,application/json" onchange={(event) => void handleAnimationUploadInput(event)} />

        {#if animationFormError}
          <p class="mt-2 rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-300">{animationFormError}</p>
        {/if}
        {#if animationFormStatus}
          <p class="mt-2 rounded border border-success-500/40 bg-success-500/10 px-3 py-2 text-xs text-success-300">
            {animationFormStatus}
          </p>
        {/if}
      </div>

      <div class="space-y-3">
        <LightingCompactEditorPanel
          {ledColors}
          {ledWhites}
          ledBrightnesses={ledSelectionBrightness}
          {selectedLedIndices}
          {editorColor}
          {editorBrightness}
          previewBusy={liveBusy || sequenceBusy}
          stopBusy={liveBusy}
          onToggleLed={(index) => toggleLedSelection(index)}
          onEditorColorChange={(value) => handleEditorColorChange(value)}
          onEditorBrightnessChange={(value) => handleEditorBrightnessChange(value)}
          onPreview={() => void previewCurrentFrameOnDevice()}
          onStop={() => void stopDeviceOutput()}
          onSelectAll={() => selectAllLeds()}
          onClearSelection={() => clearLedSelection()}
        />

        <LightingCustomSequencePanel
          {timelineKeyframes}
          {selectedTimelineKeyframeId}
          {timelineCursorMs}
          {timelineDurationMs}
          {timelineSampleMs}
          {timelineLoop}
          {sequenceBusy}
          {sequenceError}
          onTimelineCursorChange={(value) => setTimelineCursorMs(value, true)}
          onTimelineLoopChange={(value) => (timelineLoop = value)}
          onTimelineDurationChange={(value) => (timelineDurationMs = Math.max(100, Math.trunc(value || 0)))}
          onTimelineSampleChange={(value) => (timelineSampleMs = Math.max(20, Math.trunc(value || 0)))}
          onInsertKeyframe={() => addTimelineKeyframe()}
          onLoadKeyframe={(frame) => loadTimelineKeyframe(frame)}
          onRemoveKeyframe={(id) => removeTimelineKeyframe(id)}
          onSelectTimelineKeyframe={(id) => (selectedTimelineKeyframeId = id)}
          onUpdateKeyframeTime={(id, value) => updateTimelineKeyframeTime(id, value)}
          onUpdateKeyframeEasing={(id, easing) => updateTimelineKeyframeEasing(id, easing)}
          onDuplicateKeyframe={(id) => duplicateTimelineKeyframe(id)}
          onClearTimeline={() => clearTimelineKeyframes()}
          onPreviewTimeline={() => void playTimelineOnDevice()}
          onStopTimeline={() => stopTimelineSequence()}
          {scaleHex}
        />

        {#if deviceLightingState}
          <p class="rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-xs text-surface-300">
            Device output: {describeRuntimeState(deviceLightingState)}
          </p>
        {/if}
        {#if deviceLightingState?.animation_running}
          <p class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-100">
            A live animation is active on the device. Use <span class="font-semibold">Preview</span> to push this editor frame, or <span class="font-semibold">Stop</span> to clear output.
          </p>
        {/if}
        {#if liveError}
          <p class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-300">{liveError}</p>
        {/if}
        {#if liveStatus}
          <p class="rounded border border-success-500/40 bg-success-500/10 px-3 py-2 text-xs text-success-300">{liveStatus}</p>
        {/if}

      </div>
    </div>
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
    resolveDefaultAnimationForEvent={(eventKey) => defaultAnimationRefForEvent(eventKey)}
    onDefaultAnimationChange={(eventKey, animationName) => setDefaultAnimationRefForEvent(eventKey, animationName)}
    onClose={() => (showAdvanced = false)}
    onReset={() => void resetLightingConfig()}
    onRetryLoad={() => void ensureDeviceSettings()}
    onFrequencyInput={(value) => handleFrequencyInput(value)}
    onCountInput={(value) => (lightingCount = value)}
    onSave={() => void saveLighting()}
  />
{/if}
