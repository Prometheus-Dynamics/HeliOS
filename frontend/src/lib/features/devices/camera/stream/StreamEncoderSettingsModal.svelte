<script lang="ts">
  import type { EncoderSettingsDraft } from '$lib/api/streamEncoderSettings';
  import {
    encoderSettingsSupportsQuality,
    encoderSettingsSupportsVideoControls,
    type EncoderSettingsKind
  } from '$lib/api/streamEncoderSettings';

  let {
    open = false,
    selectedEncoder,
    encoderImpl,
    encoderSettings,
    selectedEncoderSettingsKind,
    selectedEncoderDefaultsSummary,
    parseSelectedResolution,
    applyOutputScale,
    onClose
  }: {
    open: boolean;
    selectedEncoder: { name?: string | null } | null;
    encoderImpl: string | null;
    encoderSettings: EncoderSettingsDraft;
    selectedEncoderSettingsKind: EncoderSettingsKind | null;
    selectedEncoderDefaultsSummary: string | null;
    parseSelectedResolution: () => { width: number; height: number } | null;
    applyOutputScale: (divisor: number) => void;
    onClose: () => void;
  } = $props();
</script>

{#if open}
  <div class="fixed inset-0 z-40 flex items-center justify-center bg-black/70 px-4 py-6" role="dialog" aria-modal="true" aria-label="Encoder settings">
    <div class="w-full max-w-2xl rounded-lg border border-surface-800 bg-surface-950 p-5 shadow-2xl">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Encoder settings</p>
          <p class="mt-1 text-sm text-surface-300">{selectedEncoder?.name ?? 'Encoder'}</p>
          <p class="text-xs text-surface-500">{encoderImpl ?? ''}</p>
        </div>
        <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={onClose}>
          Close
        </button>
      </div>

      {#if selectedEncoderDefaultsSummary}
        <p class="mt-2 text-micro text-surface-500">{selectedEncoderDefaultsSummary}</p>
      {/if}

      <div class="mt-4 space-y-3">
        {#if encoderSettingsSupportsQuality(selectedEncoderSettingsKind)}
          <label class="text-sm">
            <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Quality</span>
            <input
              class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
              type="number"
              min="1"
              max="100"
              step="1"
              value={encoderSettings.quality ?? ''}
              oninput={(e) => (encoderSettings.quality = Number(e.currentTarget.value) || null)}
              placeholder="Auto"
            />
          </label>
        {:else if encoderSettingsSupportsVideoControls(selectedEncoderSettingsKind)}
          <div class="grid gap-3 md:grid-cols-3">
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Bitrate (bps)</span>
              <input
                class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="0"
                step="100000"
                value={encoderSettings.bitrate ?? ''}
                oninput={(e) => (encoderSettings.bitrate = Number(e.currentTarget.value) || null)}
                placeholder="4000000"
              />
            </label>
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">GOP</span>
              <input
                class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="0"
                step="1"
                value={encoderSettings.gop ?? ''}
                oninput={(e) => (encoderSettings.gop = Number(e.currentTarget.value) || null)}
                placeholder="Auto"
              />
            </label>
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Threads</span>
              <input
                class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="0"
                step="1"
                value={encoderSettings.threadCount ?? ''}
                oninput={(e) => (encoderSettings.threadCount = Number(e.currentTarget.value) || null)}
                placeholder="Auto"
              />
            </label>
          </div>
          <div class="space-y-2">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Output scale</span>
              <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => applyOutputScale(1)}>1x</button>
              <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => applyOutputScale(2)}>2x</button>
              <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => applyOutputScale(3)}>3x</button>
              <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => applyOutputScale(4)}>4x</button>
            </div>
            <p class="text-micro text-surface-500">
              {#if parseSelectedResolution()}
                Source {parseSelectedResolution()?.width}x{parseSelectedResolution()?.height} · 2x-4x is recommended for higher encode FPS.
              {:else}
                Select a stream resolution to enable scale presets.
              {/if}
            </p>
          </div>

          <div class="grid gap-3 md:grid-cols-2">
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Output width</span>
              <input
                class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="0"
                step="1"
                value={encoderSettings.outWidth ?? ''}
                oninput={(e) => (encoderSettings.outWidth = Number(e.currentTarget.value) || null)}
                placeholder="Match source"
              />
            </label>
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Output height</span>
              <input
                class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="0"
                step="1"
                value={encoderSettings.outHeight ?? ''}
                oninput={(e) => (encoderSettings.outHeight = Number(e.currentTarget.value) || null)}
                placeholder="Match source"
              />
            </label>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}
