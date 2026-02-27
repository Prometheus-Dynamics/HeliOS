<script lang="ts">
  import type {
    LightingAnimationMode,
    LightingTimelineEasing,
    SavedLightingAnimation,
    TimelineKeyframe
  } from './lightingModalUtils';
  import LightingCustomSequencePanel from './LightingCustomSequencePanel.svelte';
  import LightingSaveAnimationPanel from './LightingSaveAnimationPanel.svelte';
  import LightingSavedAnimationsPanel from './LightingSavedAnimationsPanel.svelte';

  const {
    animationKind,
    liveBusy,
    timelineKeyframes,
    selectedTimelineKeyframeId,
    timelineCursorMs,
    timelineDurationMs,
    timelineSampleMs,
    sequenceBusy,
    sequenceError,
    saveName,
    saveDurationMs,
    savedBusy,
    savedError,
    savedStatus,
    savedAnimations,
    onOpenAdvanced,
    onTimelineCursorChange,
    onTimelineDurationChange,
    onTimelineSampleChange,
    onInsertKeyframe,
    onLoadKeyframe,
    onRemoveKeyframe,
    onSelectTimelineKeyframe,
    onUpdateKeyframeTime,
    onUpdateKeyframeEasing,
    onPreviewTimeline,
    onStopTimeline,
    onSaveNameChange,
    onSaveDurationChange,
    onSave,
    onRefresh,
    onPlay,
    onDelete,
    describeSavedAnimation,
    scaleHex
  } = $props<{
    animationKind: LightingAnimationMode;
    liveBusy: boolean;
    timelineKeyframes: TimelineKeyframe[];
    selectedTimelineKeyframeId: string | null;
    timelineCursorMs: number;
    timelineDurationMs: number;
    timelineSampleMs: number;
    sequenceBusy: boolean;
    sequenceError: string | null;
    saveName: string;
    saveDurationMs: number | null;
    savedBusy: boolean;
    savedError: string | null;
    savedStatus: string | null;
    savedAnimations: SavedLightingAnimation[];
    onOpenAdvanced: () => void;
    onTimelineCursorChange: (value: number) => void;
    onTimelineDurationChange: (value: number) => void;
    onTimelineSampleChange: (value: number) => void;
    onInsertKeyframe: () => void;
    onLoadKeyframe: (frame: TimelineKeyframe) => void;
    onRemoveKeyframe: (id: string) => void;
    onSelectTimelineKeyframe: (id: string | null) => void;
    onUpdateKeyframeTime: (id: string, value: number) => void;
    onUpdateKeyframeEasing: (id: string, easing: LightingTimelineEasing) => void;
    onPreviewTimeline: () => void;
    onStopTimeline: () => void;
    onSaveNameChange: (value: string) => void;
    onSaveDurationChange: (value: number | null) => void;
    onSave: () => void;
    onRefresh: () => void;
    onPlay: (entry: SavedLightingAnimation) => void;
    onDelete: (name: string) => void;
    describeSavedAnimation: (entry: SavedLightingAnimation) => string;
    scaleHex: (hex: string, factor: number) => string;
  }>();
</script>

<div class="space-y-6">
  <header class="flex flex-wrap items-start justify-between gap-4">
    <div class="space-y-1">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Animation Workspace</p>
      <p class="text-sm text-surface-400">Blender-style keyframe timeline for LED presets.</p>
    </div>
    <button class="btn btn-2xs preset-tonal" type="button" onclick={onOpenAdvanced}>
      Advanced settings
    </button>
  </header>

  <LightingCustomSequencePanel
    {timelineKeyframes}
    {selectedTimelineKeyframeId}
    {timelineCursorMs}
    {timelineDurationMs}
    {timelineSampleMs}
    timelineLoop={false}
    {sequenceBusy}
    {sequenceError}
    onTimelineCursorChange={onTimelineCursorChange}
    onTimelineLoopChange={() => {}}
    onTimelineDurationChange={onTimelineDurationChange}
    onTimelineSampleChange={onTimelineSampleChange}
    onInsertKeyframe={onInsertKeyframe}
    onLoadKeyframe={onLoadKeyframe}
    onRemoveKeyframe={onRemoveKeyframe}
    onSelectTimelineKeyframe={onSelectTimelineKeyframe}
    onUpdateKeyframeTime={onUpdateKeyframeTime}
    onUpdateKeyframeEasing={onUpdateKeyframeEasing}
    onPreviewTimeline={onPreviewTimeline}
    onStopTimeline={onStopTimeline}
    {scaleHex}
  />

  <div class="grid gap-4 xl:grid-cols-[minmax(0,0.58fr)_minmax(0,1fr)]">
    <LightingSaveAnimationPanel
      {animationKind}
      {saveName}
      {saveDurationMs}
      {savedBusy}
      {savedError}
      {savedStatus}
      onSaveNameChange={onSaveNameChange}
      onSaveDurationChange={onSaveDurationChange}
      onSave={onSave}
    />

    <LightingSavedAnimationsPanel
      {savedAnimations}
      {savedBusy}
      {liveBusy}
      onRefresh={onRefresh}
      onPlay={onPlay}
      onDelete={onDelete}
      {describeSavedAnimation}
    />
  </div>
</div>
