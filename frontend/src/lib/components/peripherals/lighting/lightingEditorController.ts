import { SvelteSet } from 'svelte/reactivity';
import { buildErrorMessage } from '$lib/ui/errorPolicy';
import { REQUESTED_BY } from '../../../../routes/settings/api';
import type { LedConfig } from '../../../../routes/settings/types';
import {
  buildFramePayloadFrom,
  buildTimelinePayloadFromKeyframes,
  clampNumber,
  compileTimelineToFrameSequence,
  normalizeTimelineKeyframes,
  type LightingColorPayload,
  type LightingFramePayload,
  type LightingRuntimeState,
  type LightingTimelineEasing,
  type LightingTimelinePayload,
  type SavedLightingAnimation,
  type TimelineKeyframe
} from './lightingModalUtils';
import {
  animationPayloadToKeyframes,
  buildScaledColorsForOutput,
  colorPayloadToHex,
  framesPayloadToKeyframes,
  makeId,
  normalizeFramesForEditor,
  sampleTimelineFrameAt,
  timelinePayloadToKeyframes
} from './lightingTimelineState';
import {
  fetchLightingRuntimeState,
  postLightingFrame,
  stopLightingOutput
} from './lightingModalApi';

type EditorStateSnapshot = {
  ledColors: string[];
  ledWhites: number[];
  ledSelectionBrightness: number[];
  selectedLedIndices: number[];
  editorColor: string;
  editorBrightness: number;
};

export type LightingEditorControllerState = {
  form: LedConfig;
  lightingCount: number;
  ledColors: string[];
  ledWhites: number[];
  selectedLedIndices: number[];
  ledSelectionBrightness: number[];
  editorColor: string;
  editorBrightness: number;
  liveBrightness: number;
  timelineKeyframes: TimelineKeyframe[];
  timelineCursorMs: number;
  timelineDurationMs: number;
  timelineSampleMs: number;
  timelineLoop: boolean;
  selectedTimelineKeyframeId: string | null;
  sequenceBusy: boolean;
  sequenceError: string | null;
  sequenceToken: number;
  previewPrimed: boolean;
  liveStatus: string | null;
  liveError: string | null;
  liveBusy: boolean;
  editorUndoStack: EditorStateSnapshot[];
  editorRedoStack: EditorStateSnapshot[];
};

type LightingEditorControllerDeps = {
  defaultLightingCount: number;
  minFreqKhz: number;
  maxFreqKhz: number;
  editorHistoryLimit: number;
  syncFromRuntimeState: (state: LightingRuntimeState, source: 'http' | 'ws') => void;
};

export function createLightingEditorController(
  state: LightingEditorControllerState,
  deps: LightingEditorControllerDeps
) {
  function validate(): string | null {
    if (!Number.isFinite(state.form.count) || state.form.count <= 0) {
      return 'LED count must be at least 1.';
    }
    if (!Number.isFinite(state.form.gpio) || state.form.gpio < 0) {
      return 'GPIO pin must be a valid BCM pin.';
    }
    const frequency = coerceInt(state.form.frequency_hz);
    const frequencyKhzValue = Math.round(frequency / 1000);
    if (
      frequencyKhzValue < deps.minFreqKhz ||
      frequencyKhzValue > deps.maxFreqKhz
    ) {
      return `PWM/bitstream frequency must be between ${deps.minFreqKhz} and ${deps.maxFreqKhz.toLocaleString()} kHz.`;
    }
    state.form.frequency_hz = frequency;
    if (typeof state.form.brightness === 'number') {
      if (
        !Number.isFinite(state.form.brightness) ||
        state.form.brightness < 0 ||
        state.form.brightness > 255
      ) {
        return 'Brightness must be between 0 and 255.';
      }
    }
    const order = state.form.color_order.trim();
    if (!order.length) return 'Color order is required.';
    const protocol = state.form.protocol.trim();
    if (!protocol.length) return 'Protocol label is required.';
    if (state.form.label != null && !state.form.label.trim().length) {
      return 'Label cannot be empty when provided.';
    }
    return null;
  }

  function applyEditorFrame(frame: LightingColorPayload[]): void {
    const resolvedCount = Math.max(
      1,
      frame.length || state.lightingCount || deps.defaultLightingCount
    );
    const nextColors = Array.from({ length: resolvedCount }, (_, idx) =>
      colorPayloadToHex(frame[idx])
    );
    const nextWhites = Array.from({ length: resolvedCount }, (_, idx) =>
      clampNumber(frame[idx]?.w ?? 0, 0, 255)
    );
    const nextBrightnesses = Array.from({ length: resolvedCount }, () => 255);
    commitEditorChange(() => {
      state.lightingCount = resolvedCount;
      state.form.count = resolvedCount;
      state.ledColors = nextColors;
      state.ledWhites = nextWhites;
      state.ledSelectionBrightness = nextBrightnesses;
      state.selectedLedIndices = [0];
      syncEditorFromSelection();
    });
  }

  function applyEditorFrameTransient(frame: LightingColorPayload[]): void {
    const resolvedCount = Math.max(
      1,
      frame.length || state.lightingCount || deps.defaultLightingCount
    );
    state.lightingCount = resolvedCount;
    state.form.count = resolvedCount;
    state.ledColors = Array.from({ length: resolvedCount }, (_, idx) =>
      colorPayloadToHex(frame[idx])
    );
    state.ledWhites = Array.from({ length: resolvedCount }, (_, idx) =>
      clampNumber(frame[idx]?.w ?? 0, 0, 255)
    );
    state.ledSelectionBrightness = Array.from(
      { length: resolvedCount },
      () => 255
    );
    syncEditorFromSelection();
  }

  function setTimelineCursorMs(value: number, syncEditor = true): void {
    state.timelineCursorMs = Math.max(0, Math.trunc(value || 0));
    if (!syncEditor || state.sequenceBusy) return;
    const sampled = sampleTimelineFrameAt(
      state.timelineKeyframes,
      state.timelineCursorMs
    );
    if (sampled) {
      applyEditorFrameTransient(sampled);
    }
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
      ledColors: [...state.ledColors],
      ledWhites: [...state.ledWhites],
      ledSelectionBrightness: [...state.ledSelectionBrightness],
      selectedLedIndices: [...state.selectedLedIndices],
      editorColor: state.editorColor,
      editorBrightness: state.editorBrightness
    };
  }

  function editorStatesEqual(
    a: EditorStateSnapshot,
    b: EditorStateSnapshot
  ): boolean {
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
    state.ledColors = [...snapshot.ledColors];
    state.ledWhites = [...snapshot.ledWhites];
    state.ledSelectionBrightness = [...snapshot.ledSelectionBrightness];
    state.selectedLedIndices = [...snapshot.selectedLedIndices];
    state.editorColor = snapshot.editorColor;
    state.editorBrightness = snapshot.editorBrightness;
  }

  function commitEditorChange(change: () => void): void {
    const before = snapshotEditorState();
    change();
    const after = snapshotEditorState();
    if (editorStatesEqual(before, after)) return;
    state.editorUndoStack = [...state.editorUndoStack, before].slice(
      -deps.editorHistoryLimit
    );
    state.editorRedoStack = [];
  }

  function undoEditorChange(): void {
    const previous = state.editorUndoStack.at(-1);
    if (!previous) return;
    const current = snapshotEditorState();
    state.editorUndoStack = state.editorUndoStack.slice(0, -1);
    state.editorRedoStack = [...state.editorRedoStack, current].slice(
      -deps.editorHistoryLimit
    );
    restoreEditorState(previous);
  }

  function redoEditorChange(): void {
    const next = state.editorRedoStack.at(-1);
    if (!next) return;
    const current = snapshotEditorState();
    state.editorRedoStack = state.editorRedoStack.slice(0, -1);
    state.editorUndoStack = [...state.editorUndoStack, current].slice(
      -deps.editorHistoryLimit
    );
    restoreEditorState(next);
  }

  function isEditableElement(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    if (
      target.isContentEditable ||
      target.tagName === 'TEXTAREA' ||
      target.tagName === 'SELECT'
    ) {
      return true;
    }
    if (target.tagName !== 'INPUT') return false;
    const inputType = (target as HTMLInputElement).type.toLowerCase();
    return ['text', 'search', 'url', 'tel', 'password', 'email', 'number'].includes(
      inputType
    );
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
    const name = `Keyframe ${state.timelineKeyframes.length + 1}`;
    const time_ms = Math.max(0, Math.trunc(Number(state.timelineCursorMs) || 0));
    const next = normalizeTimelineKeyframes([
      ...state.timelineKeyframes,
      {
        id,
        name,
        time_ms,
        colors: [...state.ledColors],
        whites: [...state.ledWhites],
        brightnesses: [...state.ledSelectionBrightness],
        easing: 'linear'
      }
    ]);
    state.timelineKeyframes = next;
    state.timelineDurationMs = Math.max(
      state.timelineDurationMs,
      time_ms + state.timelineSampleMs
    );
    state.selectedTimelineKeyframeId = id;
  }

  function loadTimelineKeyframe(frame: TimelineKeyframe): void {
    commitEditorChange(() => {
      state.ledColors = [...frame.colors];
      state.ledWhites = [...frame.whites];
      state.ledSelectionBrightness = Array.from(
        { length: frame.colors.length },
        (_, idx) => clampNumber(frame.brightnesses?.[idx] ?? 255, 0, 255)
      );
      state.timelineCursorMs = frame.time_ms;
      state.selectedTimelineKeyframeId = frame.id;
      syncEditorFromSelection();
    });
  }

  function removeTimelineKeyframe(id: string): void {
    state.timelineKeyframes = state.timelineKeyframes.filter(
      (frame) => frame.id !== id
    );
    if (state.selectedTimelineKeyframeId === id) {
      state.selectedTimelineKeyframeId =
        state.timelineKeyframes[0]?.id ?? null;
    }
  }

  function duplicateTimelineKeyframe(id: string): void {
    const source = state.timelineKeyframes.find((frame) => frame.id === id);
    if (!source) return;
    const timeOffset = Math.max(
      20,
      Math.trunc(Number(state.timelineSampleMs) || 20)
    );
    const duplicate: TimelineKeyframe = {
      ...source,
      id: makeId(),
      name: `Keyframe ${state.timelineKeyframes.length + 1}`,
      time_ms: Math.max(0, source.time_ms + timeOffset),
      colors: [...source.colors],
      whites: [...source.whites],
      brightnesses: [
        ...(source.brightnesses ??
          Array.from({ length: source.colors.length }, () => 255))
      ]
    };
    state.timelineKeyframes = normalizeTimelineKeyframes([
      ...state.timelineKeyframes,
      duplicate
    ]);
    state.timelineDurationMs = Math.max(
      state.timelineDurationMs,
      duplicate.time_ms + timeOffset
    );
    state.selectedTimelineKeyframeId = duplicate.id;
    state.timelineCursorMs = duplicate.time_ms;
    loadTimelineKeyframe(duplicate);
  }

  function clearTimelineKeyframes(): void {
    state.timelineKeyframes = [];
    state.selectedTimelineKeyframeId = null;
    state.timelineCursorMs = 0;
  }

  function updateTimelineKeyframeTime(id: string, value: number): void {
    const safeValue = Math.max(0, Math.trunc(Number(value) || 0));
    state.timelineKeyframes = normalizeTimelineKeyframes(
      state.timelineKeyframes.map((frame) =>
        frame.id === id ? { ...frame, time_ms: safeValue } : frame
      )
    );
    state.timelineDurationMs = Math.max(
      state.timelineDurationMs,
      ...state.timelineKeyframes.map((frame) =>
        frame.id === id ? safeValue : frame.time_ms
      )
    );
  }

  function updateTimelineKeyframeEasing(
    id: string,
    easing: LightingTimelineEasing
  ): void {
    state.timelineKeyframes = state.timelineKeyframes.map((frame) =>
      frame.id === id ? { ...frame, easing } : frame
    );
  }

  function buildTimelinePayloadForEditor(
    durationOverride: number | null = null
  ): LightingTimelinePayload {
    const durationValue = Number(durationOverride);
    const resolvedDuration = Number.isFinite(durationValue)
      ? Math.max(0, Math.trunc(durationValue))
      : state.timelineDurationMs;
    return buildTimelinePayloadFromKeyframes(state.timelineKeyframes, {
      durationMs: resolvedDuration,
      sampleMs: state.timelineSampleMs
    });
  }

  function stopTimelineSequence(): void {
    state.sequenceToken += 1;
    state.sequenceBusy = false;
    state.liveBusy = false;
  }

  async function playFrameSequence(
    frames: LightingFramePayload[],
    brightness: number,
    label: string,
    options?: { loop?: boolean; updateCursor?: boolean }
  ): Promise<void> {
    state.sequenceError = null;
    state.liveError = null;
    state.liveStatus = null;
    state.sequenceBusy = true;
    state.liveBusy = true;
    const token = state.sequenceToken + 1;
    state.sequenceToken = token;
    const loopPlayback = options?.loop === true;
    try {
      do {
        let loopCursor = 0;
        for (const frame of frames) {
          if (state.sequenceToken !== token) {
            break;
          }
          applyEditorFrameTransient(frame.frame ?? []);
          if (options?.updateCursor) {
            setTimelineCursorMs(loopCursor, false);
          }
          await postLightingFrame(frame.frame, brightness, REQUESTED_BY);
          const frameDuration = Math.max(50, frame.duration_ms || 0);
          await new Promise((resolve) => setTimeout(resolve, frameDuration));
          loopCursor += frameDuration;
        }
      } while (loopPlayback && state.sequenceToken === token);
      if (state.sequenceToken === token) {
        state.liveStatus = label;
      }
    } catch (err) {
      state.sequenceError = buildErrorMessage({
        error: err,
        fallback: 'Unable to preview the timeline sequence.'
      });
    } finally {
      if (state.sequenceToken === token) {
        state.sequenceBusy = false;
        state.liveBusy = false;
      }
    }
  }

  async function playTimelineOnDevice(): Promise<void> {
    if (state.timelineKeyframes.length === 0) {
      state.sequenceError = 'Insert at least one keyframe to preview a timeline.';
      return;
    }
    const timeline = buildTimelinePayloadForEditor();
    const frames = compileTimelineToFrameSequence(timeline);
    await playFrameSequence(
      frames,
      clampNumber(state.liveBrightness),
      state.timelineLoop
        ? 'Playing timeline (looping)'
        : 'Played timeline sequence',
      {
        loop: state.timelineLoop,
        updateCursor: true
      }
    );
  }

  function buildCurrentFramePayloadForOutput() {
    const scaledColors = buildScaledColorsForOutput(
      state.ledColors,
      state.ledSelectionBrightness
    );
    return buildFramePayloadFrom(scaledColors, state.ledWhites);
  }

  async function previewCurrentFrameOnDevice(): Promise<void> {
    state.liveBusy = true;
    state.liveStatus = null;
    state.liveError = null;
    try {
      const frame = buildCurrentFramePayloadForOutput();
      const brightness = clampNumber(state.liveBrightness);
      await postLightingFrame(frame, brightness, REQUESTED_BY);
      if (!state.previewPrimed) {
        state.previewPrimed = true;
        await postLightingFrame(frame, brightness, REQUESTED_BY);
      }
      state.liveStatus = 'Previewed current frame';
    } catch (err) {
      state.liveError = buildErrorMessage({
        error: err,
        fallback: 'Unable to preview current frame.'
      });
    } finally {
      state.liveBusy = false;
    }
  }

  async function stopDeviceOutput(): Promise<void> {
    stopTimelineSequence();
    state.liveBusy = true;
    state.liveError = null;
    state.liveStatus = null;
    try {
      const offFrame = Array.from(
        {
          length: Math.max(
            1,
            state.lightingCount || deps.defaultLightingCount
          )
        },
        () => ({ r: 0, g: 0, b: 0, w: 0 })
      );
      await stopLightingOutput(offFrame, REQUESTED_BY);
      state.liveStatus = 'Stopped device output';
      const runtimeState = await fetchLightingRuntimeState();
      deps.syncFromRuntimeState(runtimeState, 'http');
    } catch (err) {
      state.liveError = buildErrorMessage({
        error: err,
        fallback: 'Unable to stop lighting output.'
      });
    } finally {
      state.liveBusy = false;
    }
  }

  function applyAnimationEntryToEditor(entry: SavedLightingAnimation): boolean {
    if (typeof entry.brightness === 'number' && Number.isFinite(entry.brightness)) {
      state.liveBrightness = clampNumber(entry.brightness, 0, 255);
    }

    if (entry.timeline?.keyframes?.length) {
      const keyframes = timelinePayloadToKeyframes(
        entry.timeline,
        Math.max(1, state.lightingCount)
      );
      state.timelineKeyframes = keyframes;
      const sampleMs = Math.max(
        20,
        Math.trunc(
          Number(entry.timeline.sample_ms) || state.timelineSampleMs || 50
        )
      );
      state.timelineSampleMs = sampleMs;
      const lastFrameTime = keyframes[keyframes.length - 1]?.time_ms ?? 0;
      state.timelineDurationMs = Math.max(
        100,
        Math.trunc(Number(entry.timeline.duration_ms) || 0),
        lastFrameTime + sampleMs
      );
      const firstKeyframe = keyframes[0];
      state.selectedTimelineKeyframeId = firstKeyframe?.id ?? null;
      state.timelineCursorMs = firstKeyframe?.time_ms ?? 0;
      applyEditorFrame(entry.timeline.keyframes[0]?.frame ?? []);
      return true;
    }

    if (entry.frames?.length) {
      const normalizedFrames = normalizeFramesForEditor(
        entry.frames,
        Math.max(1, state.lightingCount)
      );
      const keyframes = framesPayloadToKeyframes(
        normalizedFrames,
        Math.max(1, state.lightingCount),
        state.timelineSampleMs
      );
      state.timelineKeyframes = keyframes;
      const sampleMs = Math.max(
        20,
        Math.trunc(
          Number(normalizedFrames[0]?.duration_ms) ||
            state.timelineSampleMs ||
            50
        )
      );
      state.timelineSampleMs = sampleMs;
      const totalDuration = normalizedFrames.reduce(
        (sum, frame) =>
          sum + Math.max(20, Math.trunc(Number(frame.duration_ms) || sampleMs)),
        0
      );
      const lastFrameTime = keyframes[keyframes.length - 1]?.time_ms ?? 0;
      state.timelineDurationMs = Math.max(
        100,
        Math.trunc(Number(entry.duration_ms) || 0),
        totalDuration,
        lastFrameTime + sampleMs
      );
      const firstKeyframe = keyframes[0];
      state.selectedTimelineKeyframeId = firstKeyframe?.id ?? null;
      state.timelineCursorMs = firstKeyframe?.time_ms ?? 0;
      applyEditorFrame(normalizedFrames[0]?.frame ?? []);
      return true;
    }

    if (entry.frame?.length) {
      clearTimelineKeyframes();
      applyEditorFrame(entry.frame);
      return true;
    }

    if (entry.animation) {
      const inferredCount = Math.max(
        1,
        state.lightingCount || deps.defaultLightingCount
      );
      const approximation = animationPayloadToKeyframes(
        entry.animation,
        inferredCount,
        clampNumber(entry.brightness ?? state.liveBrightness, 0, 255)
      );
      if (approximation.keyframes.length === 0) {
        return false;
      }
      state.timelineKeyframes = approximation.keyframes;
      state.timelineSampleMs = Math.max(
        20,
        Math.trunc(approximation.sampleMs || state.timelineSampleMs || 50)
      );
      const lastFrameTime =
        approximation.keyframes[approximation.keyframes.length - 1]?.time_ms ??
        0;
      state.timelineDurationMs = Math.max(
        100,
        approximation.durationMs,
        lastFrameTime + state.timelineSampleMs
      );
      const firstKeyframe = approximation.keyframes[0];
      if (firstKeyframe) {
        loadTimelineKeyframe(firstKeyframe);
      }
      return true;
    }

    return false;
  }

  function coerceInt(value: number | string): number {
    const num = typeof value === 'string' ? Number(value) : value;
    return Number.isFinite(num) ? Math.trunc(num) : 0;
  }

  function syncEditorFromSelection(): void {
    const selected = state.selectedLedIndices.filter(
      (idx) => idx >= 0 && idx < state.ledColors.length
    );
    if (!selected.length) {
      state.editorColor = state.ledColors[0] ?? '#00c8ff';
      state.editorBrightness = clampNumber(
        state.ledSelectionBrightness[0] ?? 255,
        0,
        255
      );
      return;
    }

    const primary = selected[0];
    state.editorColor = state.ledColors[primary] ?? '#00c8ff';
    const averageBrightness =
      selected.reduce(
        (sum, idx) =>
          sum +
          clampNumber(state.ledSelectionBrightness[idx] ?? 255, 0, 255),
        0
      ) / selected.length;
    state.editorBrightness = clampNumber(Math.round(averageBrightness), 0, 255);
  }

  function toggleLedSelection(index: number): void {
    commitEditorChange(() => {
      const next = clampNumber(
        index,
        0,
        Math.max(0, state.ledColors.length - 1)
      );
      const exists = state.selectedLedIndices.includes(next);
      const updated = exists
        ? state.selectedLedIndices.filter((item) => item !== next)
        : [...state.selectedLedIndices, next];
      state.selectedLedIndices = Array.from(new SvelteSet(updated)).sort(
        (a, b) => a - b
      );
      syncEditorFromSelection();
    });
  }

  function selectAllLeds(): void {
    commitEditorChange(() => {
      state.selectedLedIndices = Array.from(
        { length: state.ledColors.length },
        (_, idx) => idx
      );
      syncEditorFromSelection();
    });
  }

  function clearLedSelection(): void {
    commitEditorChange(() => {
      state.selectedLedIndices = [];
      syncEditorFromSelection();
    });
  }

  function handleEditorColorChange(value: string): void {
    commitEditorChange(() => {
      state.editorColor = value;
      if (state.selectedLedIndices.length === 0) return;
      const selectedSet = new SvelteSet(
        state.selectedLedIndices.map((idx) =>
          clampNumber(idx, 0, Math.max(0, state.ledColors.length - 1))
        )
      );
      state.ledColors = state.ledColors.map((current, i) =>
        selectedSet.has(i) ? value : current
      );
    });
  }

  function handleEditorBrightnessChange(value: number): void {
    commitEditorChange(() => {
      const next = clampNumber(value, 0, 255);
      state.editorBrightness = next;
      if (state.selectedLedIndices.length === 0) return;
      const selectedSet = new SvelteSet(
        state.selectedLedIndices.map((idx) =>
          clampNumber(idx, 0, Math.max(0, state.ledSelectionBrightness.length - 1))
        )
      );
      state.ledSelectionBrightness = state.ledSelectionBrightness.map(
        (current, i) => (selectedSet.has(i) ? next : current)
      );
    });
  }

  return {
    validate,
    applyAnimationEntryToEditor,
    applyEditorFrame,
    applyEditorFrameTransient,
    buildCurrentFramePayloadForOutput,
    buildTimelinePayloadForEditor,
    clearLedSelection,
    clearTimelineKeyframes,
    handleEditorBrightnessChange,
    handleEditorColorChange,
    handleEditorKeydown,
    loadTimelineKeyframe,
    playTimelineOnDevice,
    previewCurrentFrameOnDevice,
    redoEditorChange,
    removeTimelineKeyframe,
    selectAllLeds,
    setTimelineCursorMs,
    stopDeviceOutput,
    stopTimelineSequence,
    syncEditorFromSelection,
    toggleLedSelection,
    undoEditorChange,
    updateTimelineKeyframeEasing,
    updateTimelineKeyframeTime,
    addTimelineKeyframe,
    duplicateTimelineKeyframe
  };
}
