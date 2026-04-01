<script lang="ts">
  import {
    encoderSettingsKindForCodec,
    encoderSettingsSummary,
    encoderSettingsSupportsQuality,
    encoderSettingsSupportsVideoControls,
    type EncoderSettingsDraft
  } from '$lib/api/streamEncoderSettings';
  import type { CodecInfo } from '$lib/ts-bindings/http/client';
  import ModalShell from '$lib/components/ui/ModalShell.svelte';

  type SourceResolution = { width: number; height: number } | null;

  const props = $props<{
    open: boolean;
    encoderSettings: EncoderSettingsDraft;
    sourceResolution?: SourceResolution;
    currentEncoder: () => CodecInfo | null;
    onClose: () => void;
  }>();

  const encoderSettingsKind = $derived.by(() => encoderSettingsKindForCodec(props.currentEncoder()));
  const defaultsSummary = $derived.by(() => encoderSettingsSummary(props.currentEncoder()?.tunables?.encoder_settings ?? null));

  const normalizedSourceResolution = $derived.by(() => {
    const width = Number(props.sourceResolution?.width ?? 0);
    const height = Number(props.sourceResolution?.height ?? 0);
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return null;
    return { width: Math.trunc(width), height: Math.trunc(height) };
  });

  function scaledSize(value: number, divisor: number): number {
    const scaled = Math.max(16, Math.round(value / Math.max(1, divisor)));
    return scaled % 2 === 0 ? scaled : scaled - 1;
  }

  function applyOutputScale(divisor: number): void {
    const src = normalizedSourceResolution;
    if (!src) return;
    props.encoderSettings.outWidth = scaledSize(src.width, divisor);
    props.encoderSettings.outHeight = scaledSize(src.height, divisor);
  }

</script>

{#if props.open}
  <ModalShell
    open
    size="lg"
    className="z-[60]"
    panelClassName="border-surface-800 bg-surface-900/95 text-surface-50 shadow-2xl backdrop-blur"
    onClose={props.onClose}
  >
    {#snippet header()}
      <div class="min-w-0">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Encoder settings</p>
        <p class="text-lg font-semibold text-surface-50">{props.currentEncoder()?.name ?? props.currentEncoder()?.implementation ?? 'Encoder'}</p>
        <p class="text-sm text-surface-400">Optional tuning. Leave blank to use codec defaults.</p>
      </div>
    {/snippet}
    {#snippet actions()}
      <button class="btn btn-ghost" type="button" onclick={props.onClose}>Close</button>
    {/snippet}
    <div class="space-y-3">
      {#if props.currentEncoder()?.tunables?.encoder_settings}
        {#if defaultsSummary}
          <p class="text-micro text-surface-500">{defaultsSummary}</p>
        {/if}
        {#if encoderSettingsSupportsQuality(encoderSettingsKind)}
          <label class="block space-y-1">
            <span class="text-xs uppercase tracking-[0.25em] text-surface-500">Quality</span>
            <input
              class="input w-full bg-surface-950"
              type="number"
              min="1"
              max="100"
              step="1"
              value={props.encoderSettings.quality ?? ''}
              oninput={(e) => (props.encoderSettings.quality = Number((e.currentTarget as HTMLInputElement).value) || null)}
              placeholder="Auto"
            />
          </label>
        {:else if encoderSettingsSupportsVideoControls(encoderSettingsKind)}
          <div class="grid gap-3 md:grid-cols-3">
            <label class="block space-y-1">
              <span class="text-xs uppercase tracking-[0.25em] text-surface-500">Bitrate (bps)</span>
              <input
                class="input w-full bg-surface-950"
                type="number"
                min="0"
                step="100000"
                value={props.encoderSettings.bitrate ?? ''}
                oninput={(e) => (props.encoderSettings.bitrate = Number((e.currentTarget as HTMLInputElement).value) || null)}
                placeholder="Auto"
              />
            </label>
            <label class="block space-y-1">
              <span class="text-xs uppercase tracking-[0.25em] text-surface-500">GOP</span>
              <input
                class="input w-full bg-surface-950"
                type="number"
                min="0"
                step="1"
                value={props.encoderSettings.gop ?? ''}
                oninput={(e) => (props.encoderSettings.gop = Number((e.currentTarget as HTMLInputElement).value) || null)}
                placeholder="Auto"
              />
            </label>
            <label class="block space-y-1">
              <span class="text-xs uppercase tracking-[0.25em] text-surface-500">Threads</span>
              <input
                class="input w-full bg-surface-950"
                type="number"
                min="0"
                step="1"
                value={props.encoderSettings.threadCount ?? ''}
                oninput={(e) => (props.encoderSettings.threadCount = Number((e.currentTarget as HTMLInputElement).value) || null)}
                placeholder="Auto"
              />
            </label>
          </div>

          <div class="grid gap-3 md:grid-cols-2">
            <label class="block space-y-1">
              <span class="text-xs uppercase tracking-[0.25em] text-surface-500">Framerate numerator</span>
              <input
                class="input w-full bg-surface-950"
                type="number"
                min="0"
                step="1"
                value={props.encoderSettings.framerateNum ?? ''}
                oninput={(e) => (props.encoderSettings.framerateNum = Number((e.currentTarget as HTMLInputElement).value) || null)}
                placeholder="Leave blank"
              />
            </label>
            <label class="block space-y-1">
              <span class="text-xs uppercase tracking-[0.25em] text-surface-500">Framerate denominator</span>
              <input
                class="input w-full bg-surface-950"
                type="number"
                min="0"
                step="1"
                value={props.encoderSettings.framerateDen ?? ''}
                oninput={(e) => (props.encoderSettings.framerateDen = Number((e.currentTarget as HTMLInputElement).value) || null)}
                placeholder="Leave blank"
              />
            </label>
          </div>

          <div class="space-y-2">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-xs uppercase tracking-[0.25em] text-surface-500">Output scale</span>
              <button class="btn btn-xs preset-tonal" type="button" onclick={() => applyOutputScale(1)} disabled={!normalizedSourceResolution}>1x</button>
              <button class="btn btn-xs preset-tonal" type="button" onclick={() => applyOutputScale(2)} disabled={!normalizedSourceResolution}>2x</button>
              <button class="btn btn-xs preset-tonal" type="button" onclick={() => applyOutputScale(3)} disabled={!normalizedSourceResolution}>3x</button>
              <button class="btn btn-xs preset-tonal" type="button" onclick={() => applyOutputScale(4)} disabled={!normalizedSourceResolution}>4x</button>
            </div>
            <p class="text-micro text-surface-500">
              {#if normalizedSourceResolution}
                Source {normalizedSourceResolution.width}x{normalizedSourceResolution.height} · 2x-4x is recommended for higher encode FPS.
              {:else}
                Select a format/resolution to enable scaling presets.
              {/if}
            </p>
          </div>

          <div class="grid gap-3 md:grid-cols-2">
            <label class="block space-y-1">
              <span class="text-xs uppercase tracking-[0.25em] text-surface-500">Output width</span>
              <input
                class="input w-full bg-surface-950"
                type="number"
                min="0"
                step="1"
                value={props.encoderSettings.outWidth ?? ''}
                oninput={(e) => (props.encoderSettings.outWidth = Number((e.currentTarget as HTMLInputElement).value) || null)}
                placeholder="Auto"
              />
            </label>
            <label class="block space-y-1">
              <span class="text-xs uppercase tracking-[0.25em] text-surface-500">Output height</span>
              <input
                class="input w-full bg-surface-950"
                type="number"
                min="0"
                step="1"
                value={props.encoderSettings.outHeight ?? ''}
                oninput={(e) => (props.encoderSettings.outHeight = Number((e.currentTarget as HTMLInputElement).value) || null)}
                placeholder="Auto"
              />
            </label>
          </div>
        {/if}
      {:else}
        <div class="rounded border border-surface-800 bg-surface-950/60 px-4 py-3 text-sm text-surface-300">
          No encoder settings available for this codec.
        </div>
      {/if}
    </div>
  </ModalShell>
{/if}
