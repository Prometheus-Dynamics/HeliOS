<script lang="ts">
  import { browser } from '$app/environment';
  import ImuAxesGraph from '$lib/features/systems/components/ImuAxesGraph.svelte';
  import type { ImuAxes, ImuStatus } from '$lib/types/systems';
  import { createLazySvelteComponentLoader } from '$lib/utils/lazySvelteComponent';

  type ImuStatusBadge = { label: string; tone: 'success' | 'warning' | 'error' | 'muted' };
  type ImuOrientationViewerComponent = (typeof import('$lib/components/ImuOrientationViewer.svelte'))['default'];

  const imuOrientationViewerLoader = createLazySvelteComponentLoader<ImuOrientationViewerComponent>(
    () => import('$lib/components/ImuOrientationViewer.svelte')
  );

  type Props = {
    imu: ImuStatus;
    imuLoading: boolean;
    imuError: string | null;
    imuStatusBadge: ImuStatusBadge;
    tabErrorsImu: string | null | undefined;
    isApplyingImuConfig: boolean;
    imuFusionChoice: string;
    imuRangeChoice: string;
    imuIntervalChoice: number;
    imuDrVelocityDampTauChoice: number;
    imuDrStillVelocityZeroTauChoice: number;
    imuDrMaxAccelWorldChoice: number;
    imuDrMaxSpeedChoice: number;
    imuDrMaxPositionChoice: number;
    imuDrLockPositionChoice: boolean;
    imuIntervalOptions: number[];
    imuFusionOptions: string[];
    imuRangeOptions: string[];
    imuGravityReferenceChoice: string;
    imuHasPendingChange: boolean;
    imuGraphsAutoScale: boolean;
    imuAccelSeries: ImuAxes[];
    imuGyroSeries: ImuAxes[];
    imuMagSeries: ImuAxes[];
    imuHasMag: boolean;
    imuTimestampsMs: number[];
    onRefreshImu: () => void;
    onFusionChange: (value: string) => void;
    onRangeChange: (value: string) => void;
    onIntervalChange: (value: number) => void;
    onApplyImuConfig: (payload: {
      fusion?: string;
      range?: string;
      updateIntervalMs?: number;
      drVelocityDampTauSeconds?: number;
      drStillVelocityZeroTauSeconds?: number;
      drMaxAccelWorldMps2?: number;
      drMaxSpeedMps?: number;
      drMaxPositionM?: number;
      drLockPosition?: boolean;
      gravityReferenceAxis?: string;
      snapGravity?: boolean;
    }) => void;
    onDrVelocityDampTauChange: (value: number) => void;
    onDrStillVelocityZeroTauChange: (value: number) => void;
    onDrMaxAccelWorldChange: (value: number) => void;
    onDrMaxSpeedChange: (value: number) => void;
    onDrMaxPositionChange: (value: number) => void;
    onDrLockPositionChange: (value: boolean) => void;
    onResetImuPose: () => void;
    onResetImuForm: () => void;
    onSubmitImuConfig: () => void;
    onImuFormFocus: () => void;
    onImuFormBlur: () => void;
    formatImuFusion: (value: string | null | undefined) => string;
    formatImuRange: (value: string | null | undefined) => string;
    formatAngle: (value: number | null | undefined) => string;
    formatLoadError: (value: unknown) => string;
  };

  let {
    imu,
    imuLoading,
    imuError,
    imuStatusBadge,
    tabErrorsImu,
    isApplyingImuConfig,
    imuFusionChoice,
    imuRangeChoice,
    imuIntervalChoice,
    imuDrVelocityDampTauChoice,
    imuDrStillVelocityZeroTauChoice,
    imuDrMaxAccelWorldChoice,
    imuDrMaxSpeedChoice,
    imuDrMaxPositionChoice,
    imuDrLockPositionChoice,
    imuIntervalOptions,
    imuFusionOptions,
    imuRangeOptions,
    imuGravityReferenceChoice = $bindable(),
    imuHasPendingChange,
    imuGraphsAutoScale = $bindable(),
    imuAccelSeries,
    imuGyroSeries,
    imuMagSeries,
    imuHasMag,
    imuTimestampsMs,
    onRefreshImu,
    onFusionChange,
    onRangeChange,
    onIntervalChange,
    onDrVelocityDampTauChange,
    onDrStillVelocityZeroTauChange,
    onDrMaxAccelWorldChange,
    onDrMaxSpeedChange,
    onDrMaxPositionChange,
    onDrLockPositionChange,
    onApplyImuConfig,
    onResetImuPose,
    onResetImuForm,
    onSubmitImuConfig,
    onImuFormFocus,
    onImuFormBlur,
    formatImuFusion,
    formatImuRange,
    formatAngle,
    formatLoadError
  }: Props = $props();

  let ImuOrientationViewerComponent = $state<ImuOrientationViewerComponent | null>(
    imuOrientationViewerLoader.current()
  );

  const formatVectorValue = (value: number | null | undefined, digits = 2): string => {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return value.toFixed(digits);
    }
    return `0.${'0'.repeat(Math.max(0, digits))}`;
  };
  const formatPercent = (value: number | null | undefined): string => {
    if (typeof value !== 'number' || !Number.isFinite(value)) return '0%';
    const clamped = Math.min(1, Math.max(0, value));
    return `${Math.round(clamped * 100)}%`;
  };

  async function ensureImuOrientationViewer(): Promise<void> {
    ImuOrientationViewerComponent ??= await imuOrientationViewerLoader.load();
  }

  $effect(() => {
    if (!browser) return;
    void ensureImuOrientationViewer();
  });

  function readSelectValue(event: Event): string | null {
    const target = event.target;
    return target instanceof HTMLSelectElement ? target.value : null;
  }

  function readInputNumber(event: Event): number | null {
    const target = event.target;
    if (!(target instanceof HTMLInputElement)) {
      return null;
    }
    const next = Number(target.value);
    return Number.isFinite(next) ? next : null;
  }

  function handleFusionSelect(event: Event): void {
    const value = readSelectValue(event);
    if (value !== null) onFusionChange(value);
  }

  function handleRangeSelect(event: Event): void {
    const value = readSelectValue(event);
    if (value !== null) onRangeChange(value);
  }

  function handleIntervalInput(event: Event): void {
    const value = readInputNumber(event);
    if (value !== null) onIntervalChange(value);
  }

  function handleDrLockPositionSelect(event: Event): void {
    const value = readSelectValue(event);
    if (value !== null) onDrLockPositionChange(value === 'locked');
  }

  function handleDrVelocityDampTauInput(event: Event): void {
    const value = readInputNumber(event);
    if (value !== null) onDrVelocityDampTauChange(value);
  }

  function handleDrStillVelocityZeroTauInput(event: Event): void {
    const value = readInputNumber(event);
    if (value !== null) onDrStillVelocityZeroTauChange(value);
  }

  function handleDrMaxAccelWorldInput(event: Event): void {
    const value = readInputNumber(event);
    if (value !== null) onDrMaxAccelWorldChange(value);
  }

  function handleDrMaxSpeedInput(event: Event): void {
    const value = readInputNumber(event);
    if (value !== null) onDrMaxSpeedChange(value);
  }

  function handleDrMaxPositionInput(event: Event): void {
    const value = readInputNumber(event);
    if (value !== null) onDrMaxPositionChange(value);
  }

</script>

<div class="flex min-h-0 flex-1 flex-col gap-3 rounded border border-surface-800 bg-surface-950/30 p-3">
  {#if imuLoading}
    <div class="rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2 text-xs text-surface-400">
      Loading IMU telemetry…
    </div>
  {:else if imuError}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
      {imuError}
    </div>
  {:else if imuStatusBadge.tone !== 'success'}
    <div class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-100">
      {imuStatusBadge.label === 'Idle' ? 'IMU runtime has not produced samples yet.' : 'IMU telemetry unavailable.'} Try:
      <ul class="mt-1 list-disc space-y-0.5 pl-5 text-warning-50/90">
        <li>Rescan I2C buses to confirm the IMU is detected.</li>
        <li>Restart Sensors from the Settings page if the runtime is stopped.</li>
        <li>Verify wiring/power and configured bus/address in the device preset.</li>
      </ul>
    </div>
  {/if}

  <div class="grid min-h-0 flex-1 gap-3 xl:grid-cols-12">
    <section class="xl:col-span-8 flex min-h-0 flex-col rounded border border-surface-800/70 bg-surface-900/40 p-3">
      <div class="flex min-h-0 flex-1 flex-col gap-3">
        <div class="relative min-h-[17rem] min-w-0 flex-1 overflow-hidden">
          <button
            type="button"
            class="absolute top-2 right-2 z-10 rounded border border-surface-700/90 bg-surface-900/85 px-2.5 py-1.5 text-[0.65rem] font-semibold uppercase tracking-[0.2em] text-surface-100 shadow hover:border-surface-500 disabled:cursor-not-allowed disabled:opacity-60"
            onclick={() => void onResetImuPose()}
            disabled={isApplyingImuConfig || !imu.hasSample}
          >
            Reset position
          </button>
          {#if ImuOrientationViewerComponent}
            <ImuOrientationViewerComponent orientation={imu.orientation} imu={imu} />
          {:else}
            <div class="flex h-full min-h-[17rem] items-center justify-center rounded border border-surface-800/60 bg-surface-950/35 text-xs text-surface-500">
              Loading IMU viewer…
            </div>
          {/if}
        </div>

        <div class="grid grid-cols-3 gap-2 text-sm text-surface-200">
          <div class="rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1.5">
            <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Rotation (deg)</p>
            <p class="mt-1 font-mono text-xs tabular-nums text-surface-100">
              <span style="color: var(--axis-roll)">R {formatAngle(imu.orientation.roll)}</span>
              <span class="mx-2 text-surface-600">|</span>
              <span style="color: var(--axis-pitch)">P {formatAngle(imu.orientation.pitch)}</span>
              <span class="mx-2 text-surface-600">|</span>
              <span style="color: var(--axis-yaw)">Y {formatAngle(imu.orientation.yaw)}</span>
            </p>
            <p class="mt-1 font-mono text-[0.68rem] tabular-nums text-surface-400">
              ω X {formatVectorValue(imu.angularVelocityDps.x, 3)} | Y {formatVectorValue(imu.angularVelocityDps.y, 3)} | Z {formatVectorValue(imu.angularVelocityDps.z, 3)}
            </p>
            <p class="mt-1 font-mono text-[0.68rem] tabular-nums text-surface-500">
              |ω| {formatVectorValue(imu.angularSpeedDps, 3)} dps | n {formatPercent(imu.angularSpeedNormalized)}
            </p>
          </div>
          <div class="rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1.5">
            <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Position (m)</p>
            <p class="mt-1 font-mono text-xs tabular-nums text-surface-100">
              <span style="color: var(--axis-roll)">X {formatVectorValue(imu.positionWorld.x, 4)}</span>
              <span class="mx-2 text-surface-600">|</span>
              <span style="color: var(--axis-pitch)">Y {formatVectorValue(imu.positionWorld.y, 4)}</span>
              <span class="mx-2 text-surface-600">|</span>
              <span style="color: var(--axis-yaw)">Z {formatVectorValue(imu.positionWorld.z, 4)}</span>
            </p>
          </div>
          <div class="rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1.5">
            <div class="flex items-center justify-between">
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Velocity (m/s)</p>
              <p class="font-mono text-[0.7rem] tabular-nums text-surface-500">
                |v| {formatVectorValue(imu.linearSpeedMps, 4)} | n {formatPercent(imu.linearSpeedNormalized)}
              </p>
            </div>
            <p class="mt-1 font-mono text-xs tabular-nums text-surface-100">
              <span style="color: var(--axis-roll)">X {formatVectorValue(imu.velocityWorld.x, 4)}</span>
              <span class="mx-2 text-surface-600">|</span>
              <span style="color: var(--axis-pitch)">Y {formatVectorValue(imu.velocityWorld.y, 4)}</span>
              <span class="mx-2 text-surface-600">|</span>
              <span style="color: var(--axis-yaw)">Z {formatVectorValue(imu.velocityWorld.z, 4)}</span>
            </p>
            <p class="mt-1 font-mono text-[0.68rem] tabular-nums text-surface-400">
              Δv X {formatVectorValue(imu.velocityDeltaWorld.x, 4)} | Y {formatVectorValue(imu.velocityDeltaWorld.y, 4)} | Z {formatVectorValue(imu.velocityDeltaWorld.z, 4)}
            </p>
            <p class="mt-1 font-mono text-[0.68rem] tabular-nums text-surface-400">
              a(world) X {formatVectorValue(imu.correctedWorldAccelMps2.x, 3)} | Y {formatVectorValue(imu.correctedWorldAccelMps2.y, 3)} | Z {formatVectorValue(imu.correctedWorldAccelMps2.z, 3)}
            </p>
            <p class="mt-1 font-mono text-[0.68rem] tabular-nums text-surface-500">
              DR {formatPercent(imu.drConfidence)} | fast {imu.isMovingFast ? 'moving' : 'still'} | g {formatVectorValue(imu.motionFastG, 3)} / thr {formatVectorValue(imu.motionFastThresholdG, 3)}
            </p>
          </div>
        </div>
      </div>
    </section>

    <section
      class="xl:col-span-4 flex min-h-0 flex-col rounded border border-surface-800/70 bg-surface-900/40 p-3"
      onfocusin={onImuFormFocus}
      onfocusout={onImuFormBlur}
    >
      <div class="mb-3 flex items-start justify-between gap-3">
        <div>
          <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Runtime config</p>
          <p class="text-sm text-surface-300">Tune fusion and alignment</p>
          {#if imu.lastError}
            <p class="mt-1 text-xs text-error-200">Last error: {formatLoadError(imu.lastError)}</p>
          {:else if tabErrorsImu}
            <p class="mt-1 text-xs text-error-200">{tabErrorsImu}</p>
          {/if}
        </div>
        <button
          class="btn btn-3xs preset-tonal uppercase tracking-[0.25em]"
          type="button"
          onclick={onRefreshImu}
          disabled={imuLoading || isApplyingImuConfig}
        >
          Refresh
        </button>
      </div>

      <div class="min-h-0 flex-1 space-y-3 overflow-y-auto pr-1">
        <label class="block space-y-1 text-sm text-surface-200">
          <span class="text-micro uppercase tracking-[0.25em] text-surface-500">Fusion method</span>
          <select
            class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-sm text-surface-100"
            value={imuFusionChoice}
            onchange={handleFusionSelect}
          >
            {#each imuFusionOptions as option (option)}
              <option value={option}>{formatImuFusion(option)}</option>
            {/each}
          </select>
        </label>

        <label class="block space-y-1 text-sm text-surface-200">
          <span class="text-micro uppercase tracking-[0.25em] text-surface-500">Angle range</span>
          <select
            class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-sm text-surface-100"
            value={imuRangeChoice}
            onchange={handleRangeSelect}
          >
            {#each imuRangeOptions as option (option)}
              <option value={option}>{formatImuRange(option)}</option>
            {/each}
          </select>
        </label>

        <label class="block space-y-1 text-sm text-surface-200">
          <span class="text-micro uppercase tracking-[0.25em] text-surface-500">Update interval (ms)</span>
          <input
            class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-sm text-surface-100"
            type="number"
            min="1"
            step="1"
            list="imu-intervals"
            value={imuIntervalChoice}
            oninput={handleIntervalInput}
          />
          <datalist id="imu-intervals">
            {#each imuIntervalOptions as option (option)}
              <option value={option}>{option} ms</option>
            {/each}
          </datalist>
        </label>

        <label class="block space-y-1 text-sm text-surface-200">
          <span class="text-micro uppercase tracking-[0.25em] text-surface-500">Position lock</span>
          <select
            class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-sm text-surface-100"
            value={imuDrLockPositionChoice ? 'locked' : 'unlocked'}
            onchange={handleDrLockPositionSelect}
          >
            <option value="locked">Locked</option>
            <option value="unlocked">Unlocked</option>
          </select>
        </label>

        <details class="imu-accordion rounded border border-surface-800/70 bg-surface-950/60 p-3">
          <summary class="flex cursor-pointer list-none items-center justify-between text-micro uppercase tracking-[0.25em] text-surface-500">
            <span>Dead-reckoning tuning</span>
            <span class="imu-accordion-chevron" aria-hidden="true">▸</span>
          </summary>
          <div class="mt-2 grid grid-cols-1 gap-2">
            <label class="space-y-1 text-xs text-surface-300">
              <span class="uppercase tracking-[0.2em] text-surface-500">Velocity damp tau (s)</span>
              <input
                class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1.5 text-sm text-surface-100"
                type="number"
                min="0.1"
                max="30"
                step="0.1"
                value={imuDrVelocityDampTauChoice}
                oninput={handleDrVelocityDampTauInput}
              />
            </label>
            <label class="space-y-1 text-xs text-surface-300">
              <span class="uppercase tracking-[0.2em] text-surface-500">Still zero tau (s)</span>
              <input
                class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1.5 text-sm text-surface-100"
                type="number"
                min="0.02"
                max="2"
                step="0.01"
                value={imuDrStillVelocityZeroTauChoice}
                oninput={handleDrStillVelocityZeroTauInput}
              />
            </label>
            <label class="space-y-1 text-xs text-surface-300">
              <span class="uppercase tracking-[0.2em] text-surface-500">Max accel world (m/s²)</span>
              <input
                class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1.5 text-sm text-surface-100"
                type="number"
                min="0.5"
                max="30"
                step="0.1"
                value={imuDrMaxAccelWorldChoice}
                oninput={handleDrMaxAccelWorldInput}
              />
            </label>
            <label class="space-y-1 text-xs text-surface-300">
              <span class="uppercase tracking-[0.2em] text-surface-500">Max speed (m/s)</span>
              <input
                class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1.5 text-sm text-surface-100"
                type="number"
                min="0.1"
                max="20"
                step="0.1"
                value={imuDrMaxSpeedChoice}
                oninput={handleDrMaxSpeedInput}
              />
            </label>
            <label class="space-y-1 text-xs text-surface-300">
              <span class="uppercase tracking-[0.2em] text-surface-500">Max position (m)</span>
              <input
                class="w-full rounded border border-surface-800 bg-surface-950 px-2 py-1.5 text-sm text-surface-100"
                type="number"
                min="0.1"
                max="100"
                step="0.1"
                value={imuDrMaxPositionChoice}
                oninput={handleDrMaxPositionInput}
              />
            </label>
          </div>
        </details>

        <div class="space-y-2 rounded border border-surface-800/70 bg-surface-950/60 p-3">
          <p class="text-micro uppercase tracking-[0.25em] text-surface-500">Gravity alignment</p>
          <div class="flex flex-wrap items-center gap-2">
            <select
              class="min-w-[7rem] flex-1 rounded border border-surface-800 bg-surface-950 px-3 py-2 text-sm text-surface-100"
              bind:value={imuGravityReferenceChoice}
            >
              <option value="+x">+X</option>
              <option value="-x">-X</option>
              <option value="+y">+Y</option>
              <option value="-y">-Y</option>
              <option value="+z">+Z</option>
              <option value="-z">-Z</option>
            </select>
            <button
              type="button"
              class="rounded border border-surface-800 bg-surface-900/70 px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-surface-200 hover:border-surface-600"
              onclick={() => void onApplyImuConfig({ gravityReferenceAxis: imuGravityReferenceChoice })}
              disabled={isApplyingImuConfig || !imu.hasSample}
            >
              Align
            </button>
            <button
              type="button"
              class="rounded border border-surface-800 bg-surface-900/70 px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-surface-200 hover:border-surface-600"
              onclick={() => void onApplyImuConfig({ snapGravity: true })}
              disabled={isApplyingImuConfig || !imu.hasSample}
            >
              Snap
            </button>
          </div>
          <p class="text-[0.7rem] text-surface-500">Hold still on a flat face; rotates axes so accel points along the selected axis.</p>
        </div>
      </div>

      <div class="mt-3 flex items-center justify-between gap-2 border-t border-surface-800/70 pt-3">
        <p class="text-xs text-surface-500">{imuHasPendingChange ? 'Unsaved changes' : 'Synced with runtime'}</p>
        <div class="flex gap-2">
          <button
            type="button"
            class="rounded border border-surface-800 bg-surface-900/70 px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-surface-200 hover:border-surface-600"
            onclick={onResetImuForm}
            disabled={isApplyingImuConfig}
          >
            Reset
          </button>
          <button
            type="button"
            class={`rounded border px-4 py-2 text-xs font-semibold uppercase tracking-[0.25em] transition ${
              isApplyingImuConfig || !imuHasPendingChange
                ? 'cursor-not-allowed border-surface-800 bg-surface-900/70 text-surface-500'
                : 'border-primary-500 bg-primary-500/10 text-primary-50 hover:border-primary-400'
            }`}
            onclick={() => void onSubmitImuConfig()}
            disabled={isApplyingImuConfig || !imuHasPendingChange}
          >
            {isApplyingImuConfig ? 'Applying…' : 'Apply'}
          </button>
        </div>
      </div>
    </section>

    <section class="xl:col-span-12 flex min-h-0 flex-col rounded border border-surface-800/70 bg-surface-900/35 p-3">
      <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
        <div>
          <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Time series</p>
          <p class="text-xs text-surface-400">Recent accelerometer, gyroscope, and magnetometer signals</p>
        </div>
        <label class="flex items-center gap-2 text-xs text-surface-400">
          <input class="checkbox checkbox-xs" type="checkbox" bind:checked={imuGraphsAutoScale} />
          Autoscale IMU graphs
        </label>
      </div>

      <div class="grid gap-3 lg:grid-cols-3">
        <ImuAxesGraph title="Accelerometer (g)" unit="g" series={imuAccelSeries} timestampsMs={imuTimestampsMs} autoScale={imuGraphsAutoScale} />
        <ImuAxesGraph title="Gyroscope (deg/s)" unit="deg/s" series={imuGyroSeries} timestampsMs={imuTimestampsMs} autoScale={imuGraphsAutoScale} />
        {#if imuHasMag}
          <ImuAxesGraph title="Magnetometer" unit="uT" series={imuMagSeries} timestampsMs={imuTimestampsMs} autoScale={imuGraphsAutoScale} />
        {:else}
          <div class="flex items-center justify-center rounded border border-surface-800 bg-surface-950/50 p-4 text-sm text-surface-500">
            Magnetometer not detected
          </div>
        {/if}
      </div>
    </section>
  </div>
</div>

<style>
  .imu-accordion > summary::-webkit-details-marker {
    display: none;
  }

  .imu-accordion-chevron {
    transition: transform 140ms ease;
  }

  .imu-accordion[open] .imu-accordion-chevron {
    transform: rotate(90deg);
  }
</style>
