<script lang="ts">
  import { scaleHex } from './lightingModalUtils';
  import { describeRuntimeState } from './lightingAnimationLibrary';
  import LightingCompactEditorPanel from './LightingCompactEditorPanel.svelte';
  import LightingCustomSequencePanel from './LightingCustomSequencePanel.svelte';

  type Props = {
    state: Record<string, any>;
  };

  const { state }: Props = $props();
</script>

<div class="space-y-4">
  <div class="flex items-center justify-between px-1">
    <p class="text-xs text-surface-400">Timeline preset editor</p>
    <button
      class="btn btn-xs preset-tonal !px-2.5"
      type="button"
      onclick={() => (state.showAdvanced = true)}
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
          value={state.selectedLoadAnimationRef}
          disabled={state.savedBusy || state.templatesBusy || state.animationLoadBusy || state.animationSaveBusy}
          onchange={(event) => (state.selectedLoadAnimationRef = event.currentTarget.value)}
        >
          {#if state.visibleSavedAnimations.length === 0 && state.lightingTemplates.length === 0 && state.fallbackTemplateAnimations.length === 0}
            <option value="">No saved animations or templates</option>
          {:else}
            {#if state.visibleSavedAnimations.length > 0}
              <optgroup label="Saved Animations">
                {#each state.visibleSavedAnimations as entry (entry.name)}
                  <option value={state.encodeLoadAnimationRef('saved', entry.name)}>{entry.name}</option>
                {/each}
              </optgroup>
            {/if}
            {#if state.lightingTemplates.length > 0}
              <optgroup label="Templates">
                {#each state.lightingTemplates as template (template.template_id)}
                  <option value={state.encodeLoadAnimationRef('template', template.template_id)}>{template.name}</option>
                {/each}
                {#each state.fallbackTemplateAnimations as entry (entry.name)}
                  <option value={state.encodeLoadAnimationRef('template', entry.name)}>{entry.name}</option>
                {/each}
              </optgroup>
            {:else if state.fallbackTemplateAnimations.length > 0}
              <optgroup label="Templates">
                {#each state.fallbackTemplateAnimations as entry (entry.name)}
                  <option value={state.encodeLoadAnimationRef('template', entry.name)}>{entry.name}</option>
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
          value={state.animationName}
          disabled={state.animationSaveBusy}
          oninput={(event) => (state.animationName = event.currentTarget.value)}
        />
      </label>

      <div class="flex flex-wrap items-center gap-2 md:justify-end">
        <button
          class="btn btn-2xs preset-tonal"
          type="button"
          disabled={state.animationLoadBusy || !state.selectedLoadAnimationRef}
          onclick={() => void state.loadSelectedAnimationIntoEditor()}
        >
          {state.animationLoadBusy ? 'Loading…' : 'Load'}
        </button>
        <button
          class="btn btn-2xs preset-filled-primary-500"
          type="button"
          disabled={state.animationSaveBusy}
          onclick={() => void state.saveCurrentAnimation()}
        >
          {state.animationSaveBusy ? 'Saving…' : 'Save'}
        </button>
        <button
          class="btn btn-2xs preset-tonal"
          type="button"
          disabled={state.savedBusy || state.animationLoadBusy || state.animationSaveBusy || !state.selectedRefIsSavedEntry()}
          onclick={() => void state.deleteSelectedAnimation()}
        >
          Delete
        </button>
        <button
          class="btn btn-2xs preset-tonal"
          type="button"
          disabled={state.animationImportBusy}
          onclick={() => state.triggerAnimationUploadPicker()}
        >
          {state.animationImportBusy ? 'Importing…' : 'Import'}
        </button>
        <button class="btn btn-2xs preset-tonal" type="button" onclick={() => state.exportAnimationsPackage()}>
          Export
        </button>
      </div>
    </div>

    <input
      bind:this={state.animationUploadInput}
      class="hidden"
      type="file"
      accept=".json,application/json"
      onchange={(event) => void state.handleAnimationUploadInput(event)}
    />

    {#if state.animationFormError}
      <p class="mt-2 rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-300">{state.animationFormError}</p>
    {/if}
    {#if state.animationFormStatus}
      <p class="mt-2 rounded border border-success-500/40 bg-success-500/10 px-3 py-2 text-xs text-success-300">
        {state.animationFormStatus}
      </p>
    {/if}
  </div>

  <div class="space-y-3">
    <LightingCompactEditorPanel
      ledColors={state.ledColors}
      ledWhites={state.ledWhites}
      ledBrightnesses={state.ledSelectionBrightness}
      selectedLedIndices={state.selectedLedIndices}
      editorColor={state.editorColor}
      editorBrightness={state.editorBrightness}
      previewBusy={state.liveBusy || state.sequenceBusy}
      stopBusy={state.liveBusy}
      onToggleLed={(index) => state.toggleLedSelection(index)}
      onEditorColorChange={(value) => state.handleEditorColorChange(value)}
      onEditorBrightnessChange={(value) => state.handleEditorBrightnessChange(value)}
      onPreview={() => void state.previewCurrentFrameOnDevice()}
      onStop={() => void state.stopDeviceOutput()}
      onSelectAll={() => state.selectAllLeds()}
      onClearSelection={() => state.clearLedSelection()}
    />

    <LightingCustomSequencePanel
      timelineKeyframes={state.timelineKeyframes}
      selectedTimelineKeyframeId={state.selectedTimelineKeyframeId}
      timelineCursorMs={state.timelineCursorMs}
      timelineDurationMs={state.timelineDurationMs}
      timelineSampleMs={state.timelineSampleMs}
      timelineLoop={state.timelineLoop}
      sequenceBusy={state.sequenceBusy}
      sequenceError={state.sequenceError}
      onTimelineCursorChange={(value) => state.setTimelineCursorMs(value, true)}
      onTimelineLoopChange={(value) => (state.timelineLoop = value)}
      onTimelineDurationChange={(value) => (state.timelineDurationMs = Math.max(100, Math.trunc(value || 0)))}
      onTimelineSampleChange={(value) => (state.timelineSampleMs = Math.max(20, Math.trunc(value || 0)))}
      onInsertKeyframe={() => state.addTimelineKeyframe()}
      onLoadKeyframe={(frame) => state.loadTimelineKeyframe(frame)}
      onRemoveKeyframe={(id) => state.removeTimelineKeyframe(id)}
      onSelectTimelineKeyframe={(id) => (state.selectedTimelineKeyframeId = id)}
      onUpdateKeyframeTime={(id, value) => state.updateTimelineKeyframeTime(id, value)}
      onUpdateKeyframeEasing={(id, easing) => state.updateTimelineKeyframeEasing(id, easing)}
      onDuplicateKeyframe={(id) => state.duplicateTimelineKeyframe(id)}
      onClearTimeline={() => state.clearTimelineKeyframes()}
      onPreviewTimeline={() => void state.playTimelineOnDevice()}
      onStopTimeline={() => state.stopTimelineSequence()}
      {scaleHex}
    />

    {#if state.deviceLightingState}
      <p class="rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-xs text-surface-300">
        Device output: {describeRuntimeState(state.deviceLightingState)}
      </p>
    {/if}
    {#if state.deviceLightingState?.animation_running}
      <p class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-100">
        A live animation is active on the device. Use <span class="font-semibold">Preview</span> to push this editor frame, or <span class="font-semibold">Stop</span> to clear output.
      </p>
    {/if}
    {#if state.liveError}
      <p class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-300">{state.liveError}</p>
    {/if}
    {#if state.liveStatus}
      <p class="rounded border border-success-500/40 bg-success-500/10 px-3 py-2 text-xs text-success-300">{state.liveStatus}</p>
    {/if}
  </div>
</div>
