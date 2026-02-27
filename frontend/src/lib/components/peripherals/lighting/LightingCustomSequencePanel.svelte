<script lang="ts">
  import { onMount } from 'svelte';
  import type { LightingTimelineEasing, TimelineKeyframe } from './lightingModalUtils';

  type Props = {
    timelineKeyframes: TimelineKeyframe[];
    selectedTimelineKeyframeId: string | null;
    timelineCursorMs: number;
    timelineDurationMs: number;
    timelineSampleMs: number;
    timelineLoop: boolean;
    sequenceBusy: boolean;
    sequenceError: string | null;
    onTimelineCursorChange: (value: number) => void;
    onTimelineLoopChange: (value: boolean) => void;
    onTimelineDurationChange: (value: number) => void;
    onTimelineSampleChange: (value: number) => void;
    onInsertKeyframe: () => void;
    onLoadKeyframe: (frame: TimelineKeyframe) => void;
    onRemoveKeyframe: (id: string) => void;
    onSelectTimelineKeyframe: (id: string | null) => void;
    onUpdateKeyframeTime: (id: string, value: number) => void;
    onUpdateKeyframeEasing: (id: string, easing: LightingTimelineEasing) => void;
    onDuplicateKeyframe?: (id: string) => void;
    onClearTimeline?: () => void;
    onPreviewTimeline: () => void;
    onStopTimeline: () => void;
    scaleHex: (hex: string, factor: number) => string;
  };

  const {
    timelineKeyframes,
    selectedTimelineKeyframeId,
    timelineCursorMs,
    timelineDurationMs,
    timelineSampleMs,
    timelineLoop,
    sequenceBusy,
    sequenceError,
    onTimelineCursorChange,
    onTimelineLoopChange,
    onTimelineDurationChange,
    onTimelineSampleChange,
    onInsertKeyframe,
    onLoadKeyframe,
    onRemoveKeyframe,
    onSelectTimelineKeyframe,
    onUpdateKeyframeTime,
    onUpdateKeyframeEasing,
    onDuplicateKeyframe,
    onClearTimeline,
    onPreviewTimeline,
    onStopTimeline,
    scaleHex
  }: Props = $props();

  const easingOptions: { value: LightingTimelineEasing; label: string }[] = [
    { value: 'linear', label: 'Linear' },
    { value: 'ease_in', label: 'Ease In' },
    { value: 'ease_out', label: 'Ease Out' },
    { value: 'ease_in_out', label: 'Ease In/Out' },
    { value: 'step', label: 'Step' }
  ];

  let timelineTrackEl: HTMLDivElement | undefined;
  let panelRootEl: HTMLDivElement | undefined;
  let snapToSample = $state(true);

  const sortedKeyframes = $derived([...timelineKeyframes].sort((a, b) => a.time_ms - b.time_ms));
  const timelineMaxMs = $derived(Math.max(500, timelineDurationMs, ...sortedKeyframes.map((frame) => frame.time_ms + 1)));
  const selectedKeyframe = $derived(sortedKeyframes.find((frame) => frame.id === selectedTimelineKeyframeId) ?? null);
  const timelineSummary = $derived(`${sortedKeyframes.length} keyframes`);
  const safeSampleMs = $derived(Math.max(1, Math.trunc(timelineSampleMs || 1)));

  function quantizeTime(timeMs: number): number {
    const clamped = Math.max(0, Math.min(timelineMaxMs, Math.trunc(timeMs || 0)));
    if (!snapToSample) return clamped;
    return Math.round(clamped / safeSampleMs) * safeSampleMs;
  }

  function setCursor(timeMs: number): void {
    onTimelineCursorChange(quantizeTime(timeMs));
  }

  function markerPercent(timeMs: number): number {
    if (timelineMaxMs <= 0) return 0;
    return Math.min(100, Math.max(0, (timeMs / timelineMaxMs) * 100));
  }

  function updateCursorFromPointer(event: MouseEvent): void {
    if (!timelineTrackEl) return;
    const rect = timelineTrackEl.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (event.clientX - rect.left) / Math.max(1, rect.width)));
    const nextTime = Math.round(ratio * timelineMaxMs);
    setCursor(nextTime);
  }

  function jumpToPreviousKeyframe(): void {
    if (!sortedKeyframes.length) return;
    const current = timelineCursorMs;
    const previous = [...sortedKeyframes].reverse().find((frame) => frame.time_ms < current) ?? sortedKeyframes[0];
    setCursor(previous.time_ms);
    onSelectTimelineKeyframe(previous.id);
    onLoadKeyframe(previous);
  }

  function jumpToNextKeyframe(): void {
    if (!sortedKeyframes.length) return;
    const current = timelineCursorMs;
    const next = sortedKeyframes.find((frame) => frame.time_ms > current) ?? sortedKeyframes[sortedKeyframes.length - 1];
    setCursor(next.time_ms);
    onSelectTimelineKeyframe(next.id);
    onLoadKeyframe(next);
  }

  function selectKeyframe(frame: TimelineKeyframe): void {
    onSelectTimelineKeyframe(frame.id);
    onLoadKeyframe(frame);
  }

  function deleteSelected(): void {
    if (!selectedKeyframe) return;
    onRemoveKeyframe(selectedKeyframe.id);
    onSelectTimelineKeyframe(null);
  }

  function duplicateSelected(): void {
    if (!selectedKeyframe) return;
    if (onDuplicateKeyframe) {
      onDuplicateKeyframe(selectedKeyframe.id);
      return;
    }
    onLoadKeyframe(selectedKeyframe);
    setCursor(selectedKeyframe.time_ms + safeSampleMs);
    onInsertKeyframe();
  }

  function clearTimeline(): void {
    if (onClearTimeline) {
      onClearTimeline();
      return;
    }
    for (const frame of sortedKeyframes) {
      onRemoveKeyframe(frame.id);
    }
    onSelectTimelineKeyframe(null);
  }

  function fitDurationToTimeline(): void {
    const lastTime = sortedKeyframes[sortedKeyframes.length - 1]?.time_ms ?? 0;
    onTimelineDurationChange(Math.max(100, lastTime + safeSampleMs));
  }

  function nudgeCursor(stepCount: number): void {
    setCursor(timelineCursorMs + stepCount * safeSampleMs);
  }

  function nudgeSelected(stepCount: number): void {
    if (!selectedKeyframe) return;
    const nextTime = quantizeTime(selectedKeyframe.time_ms + stepCount * safeSampleMs);
    onUpdateKeyframeTime(selectedKeyframe.id, nextTime);
    setCursor(nextTime);
  }

  function isEditableTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT', 'BUTTON'].includes(target.tagName);
  }

  function handleTimelineHotkeys(event: KeyboardEvent): void {
    if (isEditableTarget(event.target)) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'd') {
      event.preventDefault();
      duplicateSelected();
      return;
    }
    if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault();
      deleteSelected();
      return;
    }
    if (event.key === 'ArrowLeft') {
      event.preventDefault();
      nudgeCursor(event.shiftKey ? -10 : -1);
      return;
    }
    if (event.key === 'ArrowRight') {
      event.preventDefault();
      nudgeCursor(event.shiftKey ? 10 : 1);
    }
  }

  onMount(() => {
    const onWindowKeydown = (event: KeyboardEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      const active = document.activeElement;
      const panelHasFocus = Boolean(panelRootEl && active instanceof Node && panelRootEl.contains(active));
      const targetInPanel = Boolean(panelRootEl && panelRootEl.contains(target));
      if (!panelHasFocus && !targetInPanel) return;
      handleTimelineHotkeys(event);
    };
    window.addEventListener('keydown', onWindowKeydown);
    return () => {
      window.removeEventListener('keydown', onWindowKeydown);
    };
  });
</script>

<div bind:this={panelRootEl} class="space-y-4 rounded-xl bg-surface-950/60 p-4">
  <header class="space-y-1">
    <div class="space-y-1">
      <p class="text-sm font-semibold text-surface-100">Timeline Editor</p>
      <p class="text-xs text-surface-400">Create keyframes, set easing, and preview directly on device.</p>
    </div>
    <p class="text-[0.68rem] text-surface-500">Hotkeys: Left/Right nudge cursor, Shift+Left/Right x10, Delete remove, Ctrl/Cmd+D duplicate.</p>
  </header>

  <div class="grid gap-2 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-center">
    <div class="flex flex-wrap items-center gap-2">
      <button class="btn btn-3xs preset-tonal" type="button" onclick={() => setCursor(0)}>Start</button>
      <button class="btn btn-3xs preset-tonal" type="button" onclick={() => setCursor(timelineMaxMs)}>End</button>
      <button class="btn btn-3xs preset-tonal" type="button" onclick={() => jumpToPreviousKeyframe()}>Prev Key</button>
      <button class="btn btn-3xs preset-tonal" type="button" onclick={() => jumpToNextKeyframe()}>Next Key</button>
    </div>
    <div class="flex flex-wrap items-center gap-2 lg:justify-end">
      <button class="btn btn-3xs preset-filled-primary-500" type="button" onclick={() => onInsertKeyframe()}>Add Keyframe</button>
      <button class="btn btn-3xs preset-tonal" type="button" disabled={!selectedKeyframe} onclick={() => duplicateSelected()}>Duplicate</button>
      <button class="btn btn-3xs preset-tonal" type="button" disabled={!selectedKeyframe} onclick={() => deleteSelected()}>Delete</button>
      <button class="btn btn-3xs preset-tonal" type="button" disabled={!sortedKeyframes.length} onclick={() => fitDurationToTimeline()}>Fit Duration</button>
      <button class="btn btn-3xs preset-tonal" type="button" disabled={!sortedKeyframes.length} onclick={() => clearTimeline()}>Clear All</button>
      <label class="flex items-center gap-1 rounded border border-surface-800/70 px-2 py-1 text-[0.68rem] text-surface-400">
        <input
          class="checkbox checkbox-xs"
          type="checkbox"
          checked={timelineLoop}
          onchange={(event) => onTimelineLoopChange((event.target as HTMLInputElement).checked)}
        />
        Loop
      </label>
      <button class="btn btn-3xs preset-tonal" type="button" disabled={sequenceBusy} onclick={() => onPreviewTimeline()}>Play</button>
      <button class="btn btn-3xs preset-outline" type="button" disabled={!sequenceBusy} onclick={() => onStopTimeline()}>Stop</button>
    </div>
  </div>

  <section class="space-y-2 rounded-lg bg-surface-950/45 p-3">
    <div class="flex items-center justify-between text-xs text-surface-400">
      <div class="flex items-center gap-3">
        <span>Cursor: {timelineCursorMs} ms</span>
        <span class="text-surface-500">{timelineSummary}</span>
      </div>
      <label class="flex items-center gap-2 text-xs text-surface-400">
        <span>Duration</span>
        <input
          class="input h-7 w-24 px-2 text-right text-xs"
          type="number"
          min="100"
          step="50"
          value={timelineDurationMs}
          oninput={(event) => onTimelineDurationChange(Number((event.target as HTMLInputElement).value))}
        />
      </label>
    </div>

    <input
      class="w-full"
      type="range"
      min="0"
      max={timelineMaxMs}
      step="1"
      value={timelineCursorMs}
      oninput={(event) => setCursor(Number((event.target as HTMLInputElement).value))}
      aria-label="Timeline cursor"
    />

    <div
      bind:this={timelineTrackEl}
      class="relative h-9 rounded border border-surface-800/70 bg-surface-900/50"
      onclick={(event) => updateCursorFromPointer(event)}
      role="button"
      tabindex="0"
      onkeydown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          updateCursorFromPointer(event as unknown as MouseEvent);
        }
      }}
      aria-label="Keyframe track"
    >
      {#if timelineMaxMs > 0}
        <div class="pointer-events-none absolute inset-y-0 left-0 w-[2px] bg-primary-300/75" style={`left:${markerPercent(timelineCursorMs)}%;`}></div>
      {/if}

      {#each sortedKeyframes as frame, frameIndex (frame.id)}
        <button
          class={`absolute top-1/2 h-4 w-4 -translate-x-1/2 -translate-y-1/2 rotate-45 border ${
            selectedTimelineKeyframeId === frame.id
              ? 'border-primary-200 bg-primary-400 shadow-[0_0_0_2px_rgba(59,130,246,0.35)]'
              : 'border-surface-200 bg-surface-600 hover:border-primary-300 hover:bg-primary-500/60'
          }`}
          style={`left:${markerPercent(frame.time_ms)}%;`}
          title={`Keyframe ${frameIndex + 1} (${frame.time_ms}ms)`}
          type="button"
          onclick={(event) => {
            event.stopPropagation();
            selectKeyframe(frame);
          }}
          aria-label={`Select keyframe ${frameIndex + 1}`}
        ></button>
      {/each}
    </div>

    <div class="flex items-center justify-between text-[11px] text-surface-500">
      <span>0</span>
      <span>{Math.round(timelineMaxMs / 2)}</span>
      <span>{timelineMaxMs}</span>
    </div>
  </section>

  <div class="grid gap-3 sm:grid-cols-1">
    <label class="space-y-1 text-xs text-surface-300">
      <div class="flex items-center justify-between">
        <span class="text-micro uppercase tracking-[0.2em] text-surface-500">Sample (ms)</span>
        <label class="flex items-center gap-1 text-[0.68rem] text-surface-500">
          <input class="checkbox checkbox-xs" type="checkbox" bind:checked={snapToSample} />
          Snap
        </label>
      </div>
      <div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto] sm:items-center">
        <input
          class="input w-full"
          type="number"
          min="20"
          step="10"
          value={timelineSampleMs}
          oninput={(event) => onTimelineSampleChange(Number((event.target as HTMLInputElement).value))}
        />
        <button class="btn btn-3xs preset-tonal" type="button" onclick={() => nudgeCursor(-1)}>-Step</button>
        <button class="btn btn-3xs preset-tonal" type="button" onclick={() => nudgeCursor(1)}>+Step</button>
      </div>
    </label>
  </div>

  <div class="space-y-3">
    <section class="space-y-2 rounded-lg bg-surface-950/45 p-3">
      <div class="flex items-center justify-between">
        <p class="text-micro uppercase tracking-[0.2em] text-surface-500">Dope Sheet</p>
        <p class="text-xs text-surface-500">{sortedKeyframes.length}</p>
      </div>

      {#if sortedKeyframes.length === 0}
        <p class="rounded border border-surface-800/70 bg-surface-950/55 px-3 py-2 text-xs text-surface-500">
          No keyframes yet.
        </p>
      {:else}
        <div class="space-y-2">
          {#each sortedKeyframes as frame, frameIndex (frame.id)}
            <button
              class={`grid w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded border px-2 py-2 text-left text-xs transition ${
                frame.id === selectedTimelineKeyframeId
                  ? 'border-primary-400/70 bg-primary-500/15 text-primary-50'
                  : 'border-surface-800/70 bg-surface-950/60 text-surface-200 hover:border-surface-600'
              }`}
              type="button"
              onclick={() => selectKeyframe(frame)}
            >
              <span class="truncate font-semibold">Keyframe {frameIndex + 1}</span>
              <span class="text-surface-500">{frame.time_ms}ms</span>
            </button>
          {/each}
        </div>
      {/if}
    </section>

    <section class="space-y-2 rounded-lg bg-surface-950/45 p-3">
      <p class="text-micro uppercase tracking-[0.2em] text-surface-500">Inspector</p>

      {#if selectedKeyframe}
        <label class="space-y-1 text-xs text-surface-300">
          <div class="flex items-center justify-between">
            <span class="text-micro uppercase tracking-[0.18em] text-surface-500">Time (ms)</span>
            <div class="flex items-center gap-1">
              <button class="btn btn-3xs preset-tonal" type="button" onclick={() => nudgeSelected(-1)}>-Step</button>
              <button class="btn btn-3xs preset-tonal" type="button" onclick={() => nudgeSelected(1)}>+Step</button>
            </div>
          </div>
          <input
            class="input w-full"
            type="number"
            min="0"
            max={timelineMaxMs}
            value={selectedKeyframe.time_ms}
            oninput={(event) => onUpdateKeyframeTime(selectedKeyframe.id, Number((event.target as HTMLInputElement).value))}
          />
        </label>

        <label class="space-y-1 text-xs text-surface-300">
          <span class="text-micro uppercase tracking-[0.18em] text-surface-500">Easing</span>
          <select
            class="input w-full"
            value={selectedKeyframe.easing}
            onchange={(event) => onUpdateKeyframeEasing(selectedKeyframe.id, (event.target as HTMLSelectElement).value as LightingTimelineEasing)}
          >
            {#each easingOptions as option (option.value)}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
        </label>

        <div class="flex flex-wrap gap-1 rounded border border-surface-800/70 bg-surface-950/50 p-2">
          {#each selectedKeyframe.colors as color, idx (idx)}
            <span
              class="h-4 w-4 rounded border border-surface-800"
              style={`background:${scaleHex(color, (selectedKeyframe.brightnesses?.[idx] ?? 255) / 255)};`}
              title={`LED ${idx + 1}`}
            ></span>
          {/each}
        </div>
      {:else}
        <p class="rounded border border-surface-800/70 bg-surface-950/55 px-3 py-2 text-xs text-surface-500">
          Select a keyframe.
        </p>
      {/if}
    </section>
  </div>

  {#if sequenceError}
    <p class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-300">{sequenceError}</p>
  {/if}
</div>
