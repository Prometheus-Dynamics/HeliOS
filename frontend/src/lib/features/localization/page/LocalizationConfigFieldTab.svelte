<script lang="ts">
  import type {
    LocalizationCustomFieldOrigin,
    LocalizationFieldOriginMode
  } from '$lib/features/localization/localizationConfig';
  import type { FieldMapSummary } from '$lib/features/localization/fieldMaps';

  type Props = {
    fieldMaps: FieldMapSummary[];
    fieldMapsLoading?: boolean;
    fieldMapsError?: string | null;
    mapUploadBusy?: boolean;
    mapUploadError?: string | null;
    fieldMapSelection?: string;
    hasActiveProfile?: boolean;
    localizationConfigLoading?: boolean;
    tagSizeInput?: string;
    tagSizeError?: string | null;
    excludedTagIdsInput?: string;
    excludedTagIdsError?: string | null;
    fieldOriginMode?: LocalizationFieldOriginMode;
    fieldOriginCustom?: LocalizationCustomFieldOrigin | null;
    snapZToGround?: boolean;
    snapRollToGround?: boolean;
    snapPitchToGround?: boolean;
    onCommitTagSize?: () => void;
    onCommitExcludedTagIds?: () => void;
    onSetFieldOriginMode?: (mode: LocalizationFieldOriginMode) => void;
    onSetFieldOriginCustomNumeric?: (field: 'x' | 'z' | 'yawDeg', value: string) => void;
    onSetSnapZToGround?: (enabled: boolean) => void;
    onSetSnapRollToGround?: (enabled: boolean) => void;
    onSetSnapPitchToGround?: (enabled: boolean) => void;
    onSetFieldMapSelection?: (value: string) => void;
    onUploadMapFile?: (file: File) => void;
  };

  let {
    fieldMaps,
    fieldMapsLoading = false,
    fieldMapsError = null,
    mapUploadBusy = false,
    mapUploadError = null,
    fieldMapSelection = $bindable(''),
    hasActiveProfile = false,
    localizationConfigLoading = false,
    tagSizeInput = $bindable(''),
    tagSizeError = null,
    excludedTagIdsInput = $bindable(''),
    excludedTagIdsError = null,
    fieldOriginMode = 'blue',
    fieldOriginCustom = { x: 0, z: 0, yawDeg: 0 },
    snapZToGround = false,
    snapRollToGround = false,
    snapPitchToGround = false,
    onCommitTagSize,
    onCommitExcludedTagIds,
    onSetFieldOriginMode,
    onSetFieldOriginCustomNumeric,
    onSetSnapZToGround,
    onSetSnapRollToGround,
    onSetSnapPitchToGround,
    onSetFieldMapSelection,
    onUploadMapFile
  }: Props = $props();

  function readInputValue(event: Event): string | null {
    const input = event.currentTarget;
    return input instanceof HTMLInputElement ? input.value : null;
  }

  function readInputChecked(event: Event): boolean | null {
    const input = event.currentTarget;
    return input instanceof HTMLInputElement ? input.checked : null;
  }

  function readSelectValue(event: Event): string | null {
    const select = event.currentTarget;
    return select instanceof HTMLSelectElement ? select.value : null;
  }

  function handleSetFieldOriginMode(event: Event) {
    const value = readSelectValue(event);
    if (value === 'blue' || value === 'red' || value === 'center' || value === 'custom') {
      onSetFieldOriginMode?.(value);
    }
  }

  function handleSetFieldOriginCustomNumeric(field: 'x' | 'z' | 'yawDeg', event: Event) {
    const value = readInputValue(event);
    if (value != null) onSetFieldOriginCustomNumeric?.(field, value);
  }

  function handleSetSnapZToGround(event: Event) {
    const checked = readInputChecked(event);
    if (checked != null) onSetSnapZToGround?.(checked);
  }

  function handleSetSnapRollToGround(event: Event) {
    const checked = readInputChecked(event);
    if (checked != null) onSetSnapRollToGround?.(checked);
  }

  function handleSetSnapPitchToGround(event: Event) {
    const checked = readInputChecked(event);
    if (checked != null) onSetSnapPitchToGround?.(checked);
  }

  function handleSetFieldMapSelection(event: Event) {
    const value = readSelectValue(event);
    if (value != null) onSetFieldMapSelection?.(value);
  }

  function handleUploadMapFile(event: Event) {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    const file = input.files?.[0] ?? null;
    if (file) onUploadMapFile?.(file);
  }
</script>

<section class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
  <div class="flex items-center justify-between">
    <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Calibration + field</p>
    <span class="text-micro-tight text-surface-500">{fieldMaps.length} maps</span>
  </div>
  <div class="mt-3 grid gap-3 lg:grid-cols-2">
    <div class="grid gap-3">
      <div class="grid gap-2">
        <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Tag size</p>
        <input class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs text-surface-100 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none" placeholder="0.03175m or 1.25in" bind:value={tagSizeInput} onchange={onCommitTagSize} disabled={!hasActiveProfile || localizationConfigLoading} />
        {#if tagSizeError}
          <p class="text-micro text-rose-200">{tagSizeError}</p>
        {/if}
        <p class="text-micro text-surface-500">Required for pose solving; field map sizes are ignored.</p>
      </div>

      <div class="grid gap-2">
        <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Excluded tag IDs</p>
        <input class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs text-surface-100 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none" placeholder="e.g. 1, 2 5" bind:value={excludedTagIdsInput} onchange={onCommitExcludedTagIds} disabled={!hasActiveProfile || localizationConfigLoading} />
        {#if excludedTagIdsError}
          <p class="text-micro text-rose-200">{excludedTagIdsError}</p>
        {/if}
        <p class="text-micro text-surface-500">Comma/space-separated IDs to ignore during localization solve.</p>
      </div>

      <div class="grid gap-2">
        <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Field origin</p>
        <select class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] text-surface-200 focus:border-primary-400 focus:outline-none" value={fieldOriginMode} disabled={!hasActiveProfile || localizationConfigLoading} onchange={handleSetFieldOriginMode}>
          <option value="blue">wpiblue</option>
          <option value="red">wpired</option>
          <option value="center">center</option>
          <option value="custom">custom (from center)</option>
        </select>
        {#if fieldOriginMode === 'custom'}
          <div class="grid gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-3">
            <label class="grid gap-1">
              <span class="uppercase tracking-[0.3em] text-surface-500">Custom X (m)</span>
              <input type="number" step="0.01" value={fieldOriginCustom?.x ?? 0} disabled={!hasActiveProfile || localizationConfigLoading} onchange={(event) => handleSetFieldOriginCustomNumeric('x', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
            </label>
            <label class="grid gap-1">
              <span class="uppercase tracking-[0.3em] text-surface-500">Custom Z (m)</span>
              <input type="number" step="0.01" value={fieldOriginCustom?.z ?? 0} disabled={!hasActiveProfile || localizationConfigLoading} onchange={(event) => handleSetFieldOriginCustomNumeric('z', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
            </label>
            <label class="grid gap-1">
              <span class="uppercase tracking-[0.3em] text-surface-500">Custom yaw (deg)</span>
              <input type="number" step="0.1" value={fieldOriginCustom?.yawDeg ?? 0} disabled={!hasActiveProfile || localizationConfigLoading} onchange={(event) => handleSetFieldOriginCustomNumeric('yawDeg', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
            </label>
          </div>
        {/if}
        <p class="text-micro text-surface-500">
          Defines the reported field pose frame for this profile, relative to field center.
        </p>
      </div>

      <div class="grid gap-2">
        <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Ground snap</p>
        <label class={`flex items-center justify-between gap-3 rounded border px-3 py-2 text-micro ${snapZToGround ? 'border-primary-500/40 bg-primary-500/10 text-primary-100' : 'border-surface-800/70 bg-surface-950/60 text-surface-300'} ${!hasActiveProfile || localizationConfigLoading ? 'opacity-60' : ''}`}>
          <span class="uppercase tracking-[0.3em]">Snap Z to ground</span>
          <input type="checkbox" checked={snapZToGround} disabled={!hasActiveProfile || localizationConfigLoading} onchange={handleSetSnapZToGround} />
        </label>
        <label class={`flex items-center justify-between gap-3 rounded border px-3 py-2 text-micro ${snapRollToGround ? 'border-primary-500/40 bg-primary-500/10 text-primary-100' : 'border-surface-800/70 bg-surface-950/60 text-surface-300'} ${!hasActiveProfile || localizationConfigLoading ? 'opacity-60' : ''}`}>
          <span class="uppercase tracking-[0.3em]">Snap roll to level</span>
          <input type="checkbox" checked={snapRollToGround} disabled={!hasActiveProfile || localizationConfigLoading} onchange={handleSetSnapRollToGround} />
        </label>
        <label class={`flex items-center justify-between gap-3 rounded border px-3 py-2 text-micro ${snapPitchToGround ? 'border-primary-500/40 bg-primary-500/10 text-primary-100' : 'border-surface-800/70 bg-surface-950/60 text-surface-300'} ${!hasActiveProfile || localizationConfigLoading ? 'opacity-60' : ''}`}>
          <span class="uppercase tracking-[0.3em]">Snap pitch to level</span>
          <input type="checkbox" checked={snapPitchToGround} disabled={!hasActiveProfile || localizationConfigLoading} onchange={handleSetSnapPitchToGround} />
        </label>
        <p class="text-micro text-surface-500">
          Constrains field-space height and/or tilt for more stable solves when tags are sparse.
        </p>
      </div>
    </div>

    <div class="grid gap-2 rounded border border-surface-800/70 bg-surface-950/60 px-3 py-3">
      <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Field map</p>
      <select class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs text-surface-100 focus:border-primary-400 focus:outline-none" bind:value={fieldMapSelection} onchange={handleSetFieldMapSelection} disabled={fieldMapsLoading || localizationConfigLoading}>
        <option value="">No field map</option>
        {#each fieldMaps as map (map.id)}
          <option value={map.id}>{map.name}</option>
        {/each}
      </select>
      <label class={`inline-flex items-center justify-center rounded-md border px-3 py-2 text-micro-tight uppercase tracking-[0.3em] transition ${mapUploadBusy || localizationConfigLoading ? 'cursor-not-allowed border-surface-800/60 text-surface-600' : 'border-surface-700/70 bg-surface-900/70 text-surface-200 hover:border-surface-500 hover:text-white'}`}>
        {mapUploadBusy ? 'Uploading…' : 'Upload map'}
        <input type="file" class="sr-only" onchange={handleUploadMapFile} disabled={mapUploadBusy || localizationConfigLoading} />
      </label>
      {#if fieldMapsError}
        <p class="text-micro text-rose-200">{fieldMapsError}</p>
      {/if}
      {#if mapUploadError}
        <p class="text-micro text-rose-200">{mapUploadError}</p>
      {/if}
    </div>
  </div>
</section>
