<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount } from 'svelte';
  import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rigDefaults';
  import type { ImuAxes, SystemsPageData } from '$lib/types/systems';
  import type { SensorOrientation } from '$lib/types/devices';
  import DeviceLogsPanel from './components/DeviceLogsPanel.svelte';
  import ConsolePanel from './components/ConsolePanel.svelte';
  import ProcessesPanel from './components/ProcessesPanel.svelte';
  import SystemsActivityTabs from '$lib/features/systems/page/SystemsActivityTabs.svelte';
  import SystemsI2cPanel from '$lib/features/systems/page/SystemsI2cPanel.svelte';
  import SystemsImuPanel from '$lib/features/systems/page/SystemsImuPanel.svelte';
  import SystemsRuntimePanel from '$lib/features/systems/page/SystemsRuntimePanel.svelte';
  import { buildActivityTabs, type ActivityTabId } from '$lib/features/systems/page/systemsPageTabs';
  import { createSystemsPageImuController } from '$lib/features/systems/page/systemsPageImuController';
  import { createSystemsPageRuntime } from '$lib/features/systems/page/systemsPageRuntime';
  import {
    buildImuStatusBadge,
    busDevices,
    busLabel,
    clonePayload,
    formatAngle,
    formatBytes,
    formatImuFusion,
    formatImuRange,
    formatLoadError,
    imuIntervalList,
    imuOptionsList
  } from '$lib/features/systems/page/systemsPageUtils';
  import { emptyImuStatus, emptySystemsRuntime } from '$lib/api/systemsPage';
  import { connectionState } from '$lib/api/connection';
  import { SvelteSet } from 'svelte/reactivity';

  const EMPTY_PAYLOAD: SystemsPageData = {
    summary: [],
    device: null,
    interfaces: [],
    sessions: [],
    rig: {
      robot: { ...DEFAULT_ROBOT_DIMENSIONS },
      cameras: []
    },
    logs: [],
    runtime: emptySystemsRuntime(),
    i2cInventory: { buses: [], devices: [] },
    imu: emptyImuStatus(),
    fetchedAt: 0,
    errorMessage: 'Systems data unavailable'
  };

  const { data } = $props<{ data: { payload: SystemsPageData } }>();
  const readPayload = () => data.payload ?? EMPTY_PAYLOAD;
  let systems = $state<SystemsPageData>(clonePayload(readPayload()));
  let loadError = $state<string | null>(readPayload().errorMessage ?? null);
  let isRefreshing = $state(false);
  let runtimeLoading = $state(false);
  let i2cLoading = $state(false);
  let imuLoading = $state(false);
  let isRescanningI2c = $state(false);
  let runtimeError = $state<string | null>(readPayload().errors?.runtime ?? null);
  let i2cError = $state<string | null>(readPayload().errors?.i2c ?? null);
  let imuError = $state<string | null>(readPayload().errors?.imu ?? null);
  let isRefreshingImu = $state(false);
  let isApplyingImuConfig = $state(false);
  let imuHistory = $state<
    Array<{
      t: number;
      accel: ImuAxes;
      gyro: ImuAxes;
      mag: ImuAxes | null;
      orientation: SensorOrientation;
    }>
  >([]);
  let lastImuTimestamp = $state<number | null>(null);
  let imuStreaming = $state(false);
  let imuStreamLastMessageAt = $state<number | null>(null);
  let imuStreamCooldownUntil = $state<number>(0);
  let imuPollingPaused = $state(false);
  let imuFusionChoice = $state<string>('unknown');
  let imuRangeChoice = $state<string>('unknown');
  let imuIntervalChoice = $state<number>(0);
  let imuDrVelocityDampTauChoice = $state<number>(6.0);
  let imuDrStillVelocityZeroTauChoice = $state<number>(0.1);
  let imuDrMaxAccelWorldChoice = $state<number>(6.0);
  let imuDrMaxSpeedChoice = $state<number>(4.0);
  let imuDrMaxPositionChoice = $state<number>(2.0);
  let imuDrLockPositionChoice = $state<boolean>(true);
  let imuGravityReferenceChoice = $state<string>('+z');
  let imuFormDirty = $state(false);
  let hasLoadedOnce = $state((readPayload().fetchedAt ?? 0) > 0);

  const device = $derived(systems.device);
  const runtime = $derived(systems.runtime ?? emptySystemsRuntime());
  const i2cInventory = $derived(systems.i2cInventory ?? { buses: [], devices: [] });
  const tabErrors = $derived(systems.errors ?? {});
  const activityTabs = $derived(buildActivityTabs(runtime));
  const imu = $derived(systems.imu ?? emptyImuStatus());
  const connectionStatus = $derived($connectionState.status);
  const isBackendUnavailable = $derived(browser && navigator.onLine === false);
  const imuFusionOptions = $derived(imuOptionsList(imu.options?.fusion, imu.fusion));
  const imuRangeOptions = $derived(imuOptionsList(imu.options?.range, imu.range));
  const imuIntervalOptions = $derived(imuIntervalList(imu.options?.intervalsMs, imu.updateIntervalMs));
  const imuStatusBadge = $derived(buildImuStatusBadge(imu, systems.errors));
  const i2cBusOrder = $derived(
    [...new SvelteSet(i2cInventory.buses.map((bus) => bus.bus).concat(i2cInventory.devices.map((d) => d.bus)))].sort((a, b) => a - b)
  );
  const hasVisibleI2cInventory = $derived(i2cInventory.buses.length > 0 || i2cInventory.devices.length > 0);
  const imuAccelSeries = $derived(imuHistory.length ? imuHistory.map((entry) => entry.accel) : imu.hasSample ? [imu.accel] : []);
  const imuGyroSeries = $derived(imuHistory.length ? imuHistory.map((entry) => entry.gyro) : imu.hasSample ? [imu.gyro] : []);
  const imuMagSeries = $derived(
    imuHistory.length
      ? imuHistory.map((entry) => entry.mag).filter((value): value is ImuAxes => Boolean(value))
      : imu.mag
        ? [imu.mag]
        : []
  );
  const imuHasMag = $derived(Boolean(imu.mag) || imuMagSeries.length > 0);
  const imuTimestampsMs = $derived(imuHistory.length ? imuHistory.map((entry) => entry.t) : []);
  let imuGraphsAutoScale = $state(true);
  let activeActivityTab = $state<ActivityTabId>('runtime');
  let stopSystemsPageRuntime: (() => void) | null = null;

  const systemsPageImuController = createSystemsPageImuController({
    readSystems: () => systems,
    setSystems: (value) => {
      systems = value;
    },
    readImu: () => imu,
    readTabErrors: () => tabErrors,
    readActiveActivityTab: () => activeActivityTab,
    readConnectionStatus: () => connectionStatus,
    readIsBackendUnavailable: () => isBackendUnavailable,
    readImuIntervalOptions: () => imuIntervalOptions,
    readIsRefreshingImu: () => isRefreshingImu,
    setIsRefreshingImu: (value) => {
      isRefreshingImu = value;
    },
    readIsApplyingImuConfig: () => isApplyingImuConfig,
    setIsApplyingImuConfig: (value) => {
      isApplyingImuConfig = value;
    },
    readImuHistory: () => imuHistory,
    setImuHistory: (value) => {
      imuHistory = value;
    },
    readLastImuTimestamp: () => lastImuTimestamp,
    setLastImuTimestamp: (value) => {
      lastImuTimestamp = value;
    },
    readImuStreaming: () => imuStreaming,
    setImuStreaming: (value) => {
      imuStreaming = value;
    },
    readImuStreamLastMessageAt: () => imuStreamLastMessageAt,
    setImuStreamLastMessageAt: (value) => {
      imuStreamLastMessageAt = value;
    },
    readImuStreamCooldownUntil: () => imuStreamCooldownUntil,
    setImuStreamCooldownUntil: (value) => {
      imuStreamCooldownUntil = value;
    },
    readImuPollingPaused: () => imuPollingPaused,
    setImuPollingPaused: (value) => {
      imuPollingPaused = value;
    },
    readImuError: () => imuError,
    setImuError: (value) => {
      imuError = value;
    },
    readImuFusionChoice: () => imuFusionChoice,
    setImuFusionChoice: (value) => {
      imuFusionChoice = value;
    },
    readImuRangeChoice: () => imuRangeChoice,
    setImuRangeChoice: (value) => {
      imuRangeChoice = value;
    },
    readImuIntervalChoice: () => imuIntervalChoice,
    setImuIntervalChoice: (value) => {
      imuIntervalChoice = value;
    },
    readImuDrVelocityDampTauChoice: () => imuDrVelocityDampTauChoice,
    setImuDrVelocityDampTauChoice: (value) => {
      imuDrVelocityDampTauChoice = value;
    },
    readImuDrStillVelocityZeroTauChoice: () => imuDrStillVelocityZeroTauChoice,
    setImuDrStillVelocityZeroTauChoice: (value) => {
      imuDrStillVelocityZeroTauChoice = value;
    },
    readImuDrMaxAccelWorldChoice: () => imuDrMaxAccelWorldChoice,
    setImuDrMaxAccelWorldChoice: (value) => {
      imuDrMaxAccelWorldChoice = value;
    },
    readImuDrMaxSpeedChoice: () => imuDrMaxSpeedChoice,
    setImuDrMaxSpeedChoice: (value) => {
      imuDrMaxSpeedChoice = value;
    },
    readImuDrMaxPositionChoice: () => imuDrMaxPositionChoice,
    setImuDrMaxPositionChoice: (value) => {
      imuDrMaxPositionChoice = value;
    },
    readImuDrLockPositionChoice: () => imuDrLockPositionChoice,
    setImuDrLockPositionChoice: (value) => {
      imuDrLockPositionChoice = value;
    },
    readImuFormDirty: () => imuFormDirty,
    setImuFormDirty: (value) => {
      imuFormDirty = value;
    }
  });

  const systemsPageRuntime = createSystemsPageRuntime({
    readSystems: () => systems,
    setSystems: (value) => {
      systems = value;
    },
    readHasLoadedOnce: () => hasLoadedOnce,
    setHasLoadedOnce: (value) => {
      hasLoadedOnce = value;
    },
    readIsRefreshing: () => isRefreshing,
    setIsRefreshing: (value) => {
      isRefreshing = value;
    },
    readIsRescanningI2c: () => isRescanningI2c,
    setIsRescanningI2c: (value) => {
      isRescanningI2c = value;
    },
    setLoadError: (value) => {
      loadError = value;
    },
    setRuntimeLoading: (value) => {
      runtimeLoading = value;
    },
    setI2cLoading: (value) => {
      i2cLoading = value;
    },
    setImuLoading: (value) => {
      imuLoading = value;
    },
    setRuntimeError: (value) => {
      runtimeError = value;
    },
    setI2cError: (value) => {
      i2cError = value;
    },
    readConnectionStatus: () => connectionStatus,
    readIsBackendUnavailable: () => isBackendUnavailable,
    applyImuStatus: systemsPageImuController.applyImuStatus
  });
  const imuHasPendingChange = $derived(
    imuFormDirty &&
      Object.keys(systemsPageImuController.buildImuConfigPayload()).length > 0
  );

  $effect(() => {
    const availableTabs = activityTabs.map((tab) => tab.id);
    if (!availableTabs.includes(activeActivityTab)) {
      activeActivityTab = 'runtime';
      systemsPageImuController.syncActiveTabRuntime();
    }
  });

  onMount(() => {
    stopSystemsPageRuntime = systemsPageRuntime.start();
    systemsPageImuController.syncActiveTabRuntime();
  });

  onDestroy(() => {
    stopSystemsPageRuntime?.();
    stopSystemsPageRuntime = null;
    systemsPageImuController.destroy();
  });

  function selectActivityTab(tab: ActivityTabId): void {
    activeActivityTab = tab;
    systemsPageImuController.syncActiveTabRuntime();
  }
</script>

<section class="flex h-full min-h-0 flex-1 flex-col gap-6 overflow-hidden">
  {#if loadError}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-4 py-2 text-sm text-error-200">
      {loadError}
    </div>
  {/if}

  <div class="grid min-h-0 flex-1 gap-6 overflow-hidden">
    <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-hidden">
        <SystemsActivityTabs
          {activityTabs}
          {activeActivityTab}
          {device}
          {formatBytes}
          onSelectTab={selectActivityTab}
        />

        <div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
          {#if activeActivityTab === 'runtime'}
            <SystemsRuntimePanel
              {runtime}
              {runtimeLoading}
              {runtimeError}
            />
          {:else if activeActivityTab === 'logs'}
            <div class="flex min-h-0 flex-1 flex-col gap-2">
              {#if tabErrors.logs}
                <div class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-100">
                  {tabErrors.logs}
                </div>
              {/if}
              <DeviceLogsPanel />
            </div>
          {:else if activeActivityTab === 'i2c'}
            <SystemsI2cPanel
              {i2cLoading}
              i2cError={hasVisibleI2cInventory ? null : i2cError}
              {isRescanningI2c}
              {i2cBusOrder}
              {i2cInventory}
              onRescan={systemsPageRuntime.rescanI2c}
              busDevices={(busId) => busDevices(i2cInventory, busId)}
              busLabel={(busId) => busLabel(i2cInventory, busId)}
            />
          {:else if activeActivityTab === 'imu'}
            <SystemsImuPanel
              {imu}
              {imuLoading}
              {imuError}
              {imuStatusBadge}
              tabErrorsImu={tabErrors.imu}
              {isApplyingImuConfig}
              {imuFusionChoice}
              {imuRangeChoice}
              {imuIntervalChoice}
              imuDrVelocityDampTauChoice={imuDrVelocityDampTauChoice}
              imuDrStillVelocityZeroTauChoice={imuDrStillVelocityZeroTauChoice}
              imuDrMaxAccelWorldChoice={imuDrMaxAccelWorldChoice}
              imuDrMaxSpeedChoice={imuDrMaxSpeedChoice}
              imuDrMaxPositionChoice={imuDrMaxPositionChoice}
              imuDrLockPositionChoice={imuDrLockPositionChoice}
              {imuIntervalOptions}
              {imuFusionOptions}
              {imuRangeOptions}
              bind:imuGravityReferenceChoice
              {imuHasPendingChange}
              bind:imuGraphsAutoScale
              {imuAccelSeries}
              {imuGyroSeries}
              {imuMagSeries}
              {imuHasMag}
              {imuTimestampsMs}
              onRefreshImu={systemsPageImuController.refreshImu}
              onFusionChange={systemsPageImuController.handleFusionChange}
              onRangeChange={systemsPageImuController.handleRangeChange}
              onIntervalChange={systemsPageImuController.handleIntervalChange}
              onDrVelocityDampTauChange={systemsPageImuController.handleDrVelocityDampTauChange}
              onDrStillVelocityZeroTauChange={systemsPageImuController.handleDrStillVelocityZeroTauChange}
              onDrMaxAccelWorldChange={systemsPageImuController.handleDrMaxAccelWorldChange}
              onDrMaxSpeedChange={systemsPageImuController.handleDrMaxSpeedChange}
              onDrMaxPositionChange={systemsPageImuController.handleDrMaxPositionChange}
              onDrLockPositionChange={systemsPageImuController.handleDrLockPositionChange}
              onApplyImuConfig={systemsPageImuController.applyImuConfig}
              onResetImuPose={systemsPageImuController.resetImuPose}
              onResetImuForm={systemsPageImuController.resetImuForm}
              onSubmitImuConfig={systemsPageImuController.submitImuConfig}
              onImuFormFocus={systemsPageImuController.handleImuFormFocus}
              onImuFormBlur={systemsPageImuController.handleImuFormBlur}
              {formatImuFusion}
              {formatImuRange}
              {formatAngle}
              {formatLoadError}
            />
          {:else}
            <div class="flex min-h-0 min-w-0 flex-1">
              {#if activeActivityTab === 'console'}
                <ConsolePanel />
              {:else}
                <ProcessesPanel />
              {/if}
            </div>
          {/if}
        </div>
      </div>
  </div>
</section>
