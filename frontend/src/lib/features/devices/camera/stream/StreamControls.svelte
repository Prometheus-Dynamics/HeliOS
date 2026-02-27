<script lang="ts">
  import { onDestroy } from 'svelte';
  import { backendFeatures } from '$lib/api/backendFeatures';

  type CodecOption = { kind: string; implementation: string; name: string };

  type StreamControlsProps = {
    backendKind?: string | null;
    decoders: CodecOption[];
    encoders: CodecOption[];
    decoderEnabled: boolean;
    encoderEnabled: boolean;
    decoderImpl: string | null;
    encoderImpl: string | null;
    decoderFpsLimit: number | null;
    decoderRotationDegrees: number | null;
    decoderMirrorHorizontal: boolean;
    shadowRecorderEnabled: boolean;
    encoderFpsLimit: number | null;
    encoderSettingsAvailable: boolean;
    hostBuffer: number;
    applying: boolean;
    onDecoderSelect: (value: string) => void;
    onDecoderFpsInput: (raw: string) => void;
    onRotationChange: (value: number) => void;
    onMirrorChange: (checked: boolean) => void;
    onShadowRecorderToggle: (checked: boolean) => void;
    onEncoderSelect: (value: string) => void;
    onEncoderFpsInput: (raw: string) => void;
    onOpenEncoderSettings: () => void;
    onHostBufferInput: (raw: string) => void;
    onApplyPreset: () => void;
  };

  const {
    backendKind = null,
    decoders,
    encoders,
    decoderEnabled,
    encoderEnabled,
    decoderImpl,
    encoderImpl,
    decoderFpsLimit,
    decoderRotationDegrees,
    decoderMirrorHorizontal,
    shadowRecorderEnabled,
    encoderFpsLimit,
    encoderSettingsAvailable,
    hostBuffer,
    applying,
    onDecoderSelect,
    onDecoderFpsInput,
    onRotationChange,
    onMirrorChange,
    onShadowRecorderToggle,
    onEncoderSelect,
    onEncoderFpsInput,
    onOpenEncoderSettings,
    onHostBufferInput,
    onApplyPreset
  }: StreamControlsProps = $props();

  const rotationOptions = [0, 90, 180, 270];
  const rotationSelection = $derived.by(() => {
    const parsed = Number(decoderRotationDegrees ?? 0);
    return Number.isFinite(parsed) ? parsed : 0;
  });
  const rotationSelectionValue = $derived.by(() => String(rotationSelection));
  const rotationHasCustom = $derived.by(() => !rotationOptions.includes(rotationSelection));

  let shadowRecorderSupported = $state(false);
  const unsubscribeBackendFeatures = backendFeatures.subscribe((value) => {
    shadowRecorderSupported = Boolean(value?.shadowRecorder);
  });
  onDestroy(() => unsubscribeBackendFeatures());

  export type $$Props = StreamControlsProps;
</script>

<div class="stream-controls grid gap-3" data-backend-kind={backendKind ?? ''}>
  <div class="control-grid grid gap-3 md:grid-cols-[minmax(0,2fr)_minmax(0,1fr)_2.5rem]">
      <label class="control-field text-sm">
        <span class="control-label text-2xs uppercase tracking-[0.3em] text-surface-500">Decoder</span>
        <select
          class="control-input w-full rounded-md border border-surface-700 bg-surface-900/70 px-3"
          value={decoderEnabled ? decoderImpl ?? '' : ''}
          onchange={(event) => onDecoderSelect((event.currentTarget as HTMLSelectElement).value)}
        >
          <option value="">Disabled</option>
          {#each decoders as dec, idx (`dec-${dec.kind}-${dec.implementation}-${idx}`)}
            <option value={dec.implementation}>{dec.name} ({dec.implementation})</option>
          {/each}
        </select>
      </label>
      <label class="control-field text-sm">
        <span class="control-label text-2xs uppercase tracking-[0.3em] text-surface-500">FPS limit</span>
        <input
          class="control-input w-full rounded-md border border-surface-700 bg-surface-900/70 px-3"
          type="number"
          min="0"
          step="0.1"
          value={decoderFpsLimit ?? ''}
          oninput={(e) => onDecoderFpsInput((e.currentTarget as HTMLInputElement).value)}
          placeholder="Off"
          disabled={!decoderEnabled}
        />
      </label>
      <div class="control-spacer hidden md:block" aria-hidden="true"></div>
    </div>
    <div class="control-grid grid gap-3 md:grid-cols-[minmax(0,2fr)_minmax(0,1fr)_2.5rem]">
      <label class="control-field text-sm">
        <span class="control-label text-2xs uppercase tracking-[0.3em] text-surface-500">Rotation</span>
        <select
          class="control-input w-full rounded-md border border-surface-700 bg-surface-900/70 px-3"
          value={rotationSelectionValue}
          onchange={(event) => onRotationChange(Number((event.currentTarget as HTMLSelectElement).value))}
          disabled={!decoderEnabled}
        >
          {#if rotationHasCustom}
            <option value={rotationSelectionValue}>{rotationSelection}°</option>
          {/if}
          <option value="0">0°</option>
          <option value="90">90°</option>
          <option value="180">180°</option>
          <option value="270">270°</option>
        </select>
      </label>
      <label class="control-field text-sm">
        <span class="control-label text-2xs uppercase tracking-[0.3em] text-surface-500">Mirror</span>
        <div class="control-toggle flex items-center gap-2 rounded-md border border-surface-700 bg-surface-900/70 px-3">
          <input
            class="h-4 w-4"
            type="checkbox"
            checked={decoderMirrorHorizontal}
            onchange={(event) => onMirrorChange((event.currentTarget as HTMLInputElement).checked)}
            disabled={!decoderEnabled}
          />
          <span class="text-xs text-surface-400">Horizontal</span>
        </div>
      </label>
      <div class="control-spacer hidden md:block" aria-hidden="true"></div>
    </div>

    <div class="control-grid grid gap-3 md:grid-cols-[minmax(0,2fr)_minmax(0,1fr)_2.5rem]">
      <label class="control-field text-sm">
        <span class="control-label text-2xs uppercase tracking-[0.3em] text-surface-500">Encoder</span>
        <select
          class="control-input w-full rounded-md border border-surface-700 bg-surface-900/70 px-3"
          value={encoderEnabled ? encoderImpl ?? '' : ''}
          onchange={(event) => onEncoderSelect((event.currentTarget as HTMLSelectElement).value)}
        >
          <option value="">Disabled</option>
          {#each encoders as enc, idx (`enc-${enc.kind}-${enc.implementation}-${idx}`)}
            <option value={(String(enc.implementation ?? '').trim() || String(enc.name ?? '').trim())}>{enc.name} ({enc.implementation})</option>
          {/each}
        </select>
      </label>
      <label class="control-field text-sm">
        <span class="control-label text-2xs uppercase tracking-[0.3em] text-surface-500">FPS limit</span>
        <input
          class="control-input w-full rounded-md border border-surface-700 bg-surface-900/70 px-3"
          type="number"
          min="0"
          step="0.1"
          value={encoderFpsLimit ?? ''}
          oninput={(e) => onEncoderFpsInput((e.currentTarget as HTMLInputElement).value)}
          placeholder="Off"
          disabled={!encoderEnabled}
        />
      </label>
      <button
        class="btn btn-sm preset-tonal control-icon-btn"
        type="button"
        onclick={onOpenEncoderSettings}
        disabled={!encoderSettingsAvailable}
        aria-disabled={!encoderSettingsAvailable}
        aria-label={encoderSettingsAvailable ? 'Encoder settings' : 'No encoder settings available'}
        title={encoderSettingsAvailable ? 'Encoder settings' : 'No encoder settings available'}
      >
        <svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
          <path
            d="M19.14 12.94c.04-.31.06-.63.06-.94s-.02-.63-.06-.94l2.03-1.58a.5.5 0 0 0 .12-.64l-1.92-3.32a.5.5 0 0 0-.6-.22l-2.39.96a7.2 7.2 0 0 0-1.63-.94l-.36-2.54A.5.5 0 0 0 13.9 1h-3.8a.5.5 0 0 0-.49.42l-.36 2.54c-.58.23-1.12.54-1.63.94l-2.39-.96a.5.5 0 0 0-.6.22L2.71 7.48a.5.5 0 0 0 .12.64l2.03 1.58c-.04.31-.06.63-.06.94s.02.63.06.94l-2.03 1.58a.5.5 0 0 0-.12.64l1.92 3.32c.13.22.39.3.6.22l2.39-.96c.5.4 1.05.71 1.63.94l.36 2.54c.04.24.25.42.49.42h3.8c.24 0 .45-.18.49-.42l.36-2.54c.58-.23 1.12-.54 1.63-.94l2.39.96c.22.09.47 0 .6-.22l1.92-3.32a.5.5 0 0 0-.12-.64l-2.03-1.58ZM12 15.5A3.5 3.5 0 1 1 12 8a3.5 3.5 0 0 1 0 7.5Z"
          />
        </svg>
      </button>
    </div>

    <label class="control-field text-sm w-full max-w-[14rem]">
      <span class="control-label text-2xs uppercase tracking-[0.3em] text-surface-500">Host buffer</span>
      <input
        class="control-input w-full rounded-md border border-surface-700 bg-surface-900/70 px-3"
        type="number"
        min="1"
        value={hostBuffer}
        oninput={(e) => onHostBufferInput((e.currentTarget as HTMLInputElement).value)}
      />
    </label>

  {#if shadowRecorderSupported}
    <label class="text-sm">
      <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Shadow recorder</span>
      <div class="mt-2 flex items-center gap-2">
        <input
          type="checkbox"
          checked={shadowRecorderEnabled}
          onchange={(event) => onShadowRecorderToggle((event.currentTarget as HTMLInputElement).checked)}
        />
        <span class="text-xs text-surface-400">Keep a rolling buffer for capture-last clips.</span>
      </div>
    </label>
  {/if}

  <div class="mt-1 flex flex-wrap items-center gap-2">
    <button
      class="btn btn-sm preset-filled-primary-500 uppercase tracking-[0.3em]"
      type="button"
      onclick={onApplyPreset}
      disabled={applying}
    >
      {applying ? 'Applying…' : 'Apply stream settings'}
    </button>
    <span class="text-2xs text-surface-500">Restarts the stream with the selected backend/pipeline/codecs.</span>
  </div>
</div>

<style>
  .control-grid {
    align-items: end;
  }

  .control-field {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .control-label {
    line-height: 1.1;
  }

  .control-input {
    height: 2.5rem;
    min-height: 2.5rem;
  }

  .control-toggle {
    height: 2.5rem;
    min-height: 2.5rem;
  }

  .control-spacer {
    height: 2.5rem;
  }

  .control-icon-btn {
    min-height: 2.5rem;
    min-width: 2.5rem;
    align-self: end;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
</style>
