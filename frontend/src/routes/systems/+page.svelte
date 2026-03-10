<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount } from 'svelte';
  import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rigDefaults';
  import type { ImuAxes, ImuStatus, SystemsPageData } from '$lib/types/systems';
  import type { SensorOrientation } from '$lib/types/devices';
  import DeviceLogsPanel from './components/DeviceLogsPanel.svelte';
  import ConsolePanel from './components/ConsolePanel.svelte';
  import ProcessesPanel from './components/ProcessesPanel.svelte';
  import { createDomainResource } from '$lib/api/domainResources';
  import { scheduleWhenIdle } from '$lib/utils/browserSchedule';
  import { startRefreshScheduler } from '$lib/api/refreshScheduler';
  import SystemsActivityTabs from '$lib/features/systems/page/SystemsActivityTabs.svelte';
  import SystemsI2cPanel from '$lib/features/systems/page/SystemsI2cPanel.svelte';
  import SystemsImuPanel from '$lib/features/systems/page/SystemsImuPanel.svelte';
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
  import { connectImuStream, emptyImuStatus, fetchI2cInventorySnapshot, refreshI2cInventory, refreshImuStatus, updateImuConfig } from '$lib/api/systemsPage';
  import { connectionState } from '$lib/api/connection';
  import { reportError } from '$lib/ui/errorPolicy';
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
  let i2cLoading = $state(false);
  let imuLoading = $state(false);
  let isRescanningI2c = $state(false);
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
  let stopImuPollLoop: (() => void) | null = null;
  let imuPollStartTimer: ReturnType<typeof setTimeout> | null = null;
  let imuStreamClose: (() => void) | null = null;
  let imuStreamReconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let imuStreaming = $state(false);
  let imuStreamDisconnecting = false;
  let imuStreamLastMessageAt = $state<number | null>(null);
  let imuStreamCooldownUntil = $state<number>(0);
  let imuPollingPaused = $state(false);
  let imuFocusTimer: ReturnType<typeof setTimeout> | null = null;
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
  let stopDomainInvalidation: (() => void) | null = null;
  let stopImuWatchdog: (() => void) | null = null;
  let cancelBootstrapRefresh: (() => void) | null = null;

  const device = $derived(systems.device);
  const i2cInventory = $derived(systems.i2cInventory ?? { buses: [], devices: [] });
  const tabErrors = $derived(systems.errors ?? {});
  const imu = $derived(systems.imu ?? emptyImuStatus());
  const connectionStatus = $derived($connectionState.status);
  const isBackendUnavailable = $derived(browser && navigator.onLine === false);
  const imuFusionOptions = $derived(imuOptionsList(imu.options?.fusion, imu.fusion));
  const imuRangeOptions = $derived(imuOptionsList(imu.options?.range, imu.range));
  const imuIntervalOptions = $derived(imuIntervalList(imu.options?.intervalsMs, imu.updateIntervalMs));
  const imuHasPendingChange = $derived(imuFormDirty && Object.keys(buildImuConfigPayload()).length > 0);
  const imuStatusBadge = $derived(buildImuStatusBadge(imu, systems.errors));
  const i2cBusOrder = $derived(
    [...new SvelteSet(i2cInventory.buses.map((bus) => bus.bus).concat(i2cInventory.devices.map((d) => d.bus)))].sort((a, b) => a - b)
  );
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
  const IMU_POLL_MS = 1000;
  const IMU_STREAM_RECONNECT_MS = 500;
  const IMU_STREAM_STALE_BASE_MS = 750;
  const IMU_STREAM_STALE_MAX_MS = 8000;
  const IMU_HISTORY_LIMIT = 180;
  const I2C_CACHE_KEY = 'systems:i2c:v1';
  const IMU_CACHE_KEY = 'systems:imu:v1';
  const SYSTEMS_CACHE_STALE_MS = 10_000;
  const SYSTEMS_CACHE_MAX_MS = 120_000;
  const i2cResource = createDomainResource({
    key: I2C_CACHE_KEY,
    loader: fetchI2cInventorySnapshot,
    staleMs: SYSTEMS_CACHE_STALE_MS,
    maxAgeMs: SYSTEMS_CACHE_MAX_MS,
    kinds: ['device', 'settings']
  });
  const imuResource = createDomainResource({
    key: IMU_CACHE_KEY,
    loader: refreshImuStatus,
    staleMs: SYSTEMS_CACHE_STALE_MS,
    maxAgeMs: SYSTEMS_CACHE_MAX_MS,
    kinds: ['device', 'imu', 'settings']
  });

  type ActivityTabId = 'logs' | 'i2c' | 'imu' | 'console' | 'processes';
  const activityTabs: Array<{ id: ActivityTabId; label: string; detail: string }> = [
    { id: 'logs', label: 'Logs', detail: 'Live service output' },
    { id: 'i2c', label: 'I2C', detail: 'Buses & devices' },
    { id: 'imu', label: 'IMU', detail: 'Orientation & axes' },
    { id: 'console', label: 'Console', detail: 'Runtime shell' },
    { id: 'processes', label: 'Processes', detail: 'CPU & memory by program' }
  ];
  let activeActivityTab = $state<ActivityTabId>('logs');

  $effect(() => {
    if (activeActivityTab === 'imu') {
      imuPollingPaused = false;
      startImuStream();
      scheduleImuPollingFallback();
    } else {
      stopImuStream();
      stopImuPolling();
    }
  });

  // Watchdog: if the IMU websocket stream stalls, reconnect quickly so the UI doesn't "pause".
  $effect(() => {
    stopImuWatchdog?.();
    stopImuWatchdog = null;
    if (activeActivityTab !== 'imu') return;
    if (!imuStreamClose) return;
    stopImuWatchdog = startRefreshScheduler(() => {
      if (activeActivityTab !== 'imu') return;
      if (!imuStreaming) return;
      if (imuPollingPaused) return;
      if (isApplyingImuConfig) return;
      const last = imuStreamLastMessageAt;
      if (last == null) return;
      if (Date.now() - last <= currentImuStreamStaleMs()) return;
      forceImuStreamReconnect();
    }, {
      intervalMs: 250,
      immediate: false,
      enabled: () => activeActivityTab === 'imu' && imuStreaming && !imuPollingPaused && !isApplyingImuConfig
    });
    return () => {
      stopImuWatchdog?.();
      stopImuWatchdog = null;
    };
  });

  $effect(() => {
    if (imu.lastError) {
      imuError = formatLoadError(imu.lastError);
    } else {
      imuError = tabErrors.imu ?? null;
    }
    recordImuSample(imu);
  });

  $effect(() => {
    if (!imuFormDirty) {
      imuFusionChoice = imu.fusion;
      imuRangeChoice = imu.range;
      const interval = imu.updateIntervalMs && imu.updateIntervalMs > 0 ? imu.updateIntervalMs : imuIntervalOptions[0] ?? 100;
      imuIntervalChoice = interval;
      imuDrVelocityDampTauChoice = imu.drVelocityDampTauSeconds;
      imuDrStillVelocityZeroTauChoice = imu.drStillVelocityZeroTauSeconds;
      imuDrMaxAccelWorldChoice = imu.drMaxAccelWorldMps2;
      imuDrMaxSpeedChoice = imu.drMaxSpeedMps;
      imuDrMaxPositionChoice = imu.drMaxPositionM;
      imuDrLockPositionChoice = imu.drLockPosition;
    }
  });

  onMount(() => {
    if (browser) {
      const cachedI2c = i2cResource.read();
      if (cachedI2c?.data) {
        systems = { ...systems, i2cInventory: cachedI2c.data, fetchedAt: cachedI2c.fetchedAt };
        i2cError = null;
      }
      const cachedImu = imuResource.read();
      if (cachedImu?.data) {
        systems = { ...systems, imu: cachedImu.data, fetchedAt: cachedImu.fetchedAt };
        imuError = cachedImu.data.lastError ? formatLoadError(cachedImu.data.lastError) : null;
      }
      if (cachedI2c?.data || cachedImu?.data) {
        hasLoadedOnce = true;
      }
    }
    if (hasLoadedOnce) {
      cancelBootstrapRefresh = scheduleWhenIdle(() => {
        if (!document.hidden) {
          void refreshSystems();
        }
      }, { timeoutMs: 1800, fallbackMs: 650 });
    } else {
      void refreshSystems({ bootstrap: true });
    }
    const stopI2cInvalidations = i2cResource.subscribeInvalidations(() => {
      void refreshSystems();
    }, { debounceMs: 250 });
    const stopImuInvalidations = imuResource.subscribeInvalidations(() => {
      void refreshSystems();
      void refreshImu();
    }, { debounceMs: 250 });
    stopDomainInvalidation = () => {
      stopI2cInvalidations();
      stopImuInvalidations();
    };
  });

  onDestroy(() => {
    cancelBootstrapRefresh?.();
    cancelBootstrapRefresh = null;
    stopDomainInvalidation?.();
    stopImuStream();
    stopImuPolling();
  });

  async function refreshSystems(options: { bootstrap?: boolean } = {}): Promise<void> {
    const { bootstrap = false } = options;
    if (!browser || isRefreshing) return;
    if (isBackendUnavailable) {
      if (bootstrap && !hasLoadedOnce) {
        loadError = 'Backend unavailable. Update the API base URL in Settings.';
        hasLoadedOnce = true;
      }
      return;
    }
    if (bootstrap) {
      loadError = null;
    }
    isRefreshing = true;
    let pending = 2;
    let failures = 0;
    let sawSuccess = false;

    const finalize = () => {
      pending -= 1;
      if (pending > 0) return;
      isRefreshing = false;
      if (bootstrap || !hasLoadedOnce) {
        hasLoadedOnce = true;
      }
      if (!sawSuccess && failures >= 2) {
        loadError = 'Systems data unavailable';
      }
    };

    i2cLoading = true;
    i2cResource.refresh()
      .then((inventory) => {
        sawSuccess = true;
        loadError = null;
        i2cLoading = false;
        i2cError = null;
        systems = {
          ...systems,
          i2cInventory: inventory,
          errors: { ...(systems.errors ?? {}), i2c: null },
          fetchedAt: Date.now()
        };
      })
      .catch((error) => {
        failures += 1;
        i2cLoading = false;
        if (connectionStatus === 'online') {
          reportError({ context: 'Systems I2C refresh', error, toast: false });
        }
        const message = formatLoadError(error);
        i2cError = message;
        systems = { ...systems, errors: { ...(systems.errors ?? {}), i2c: message } };
        i2cResource.invalidate();
      })
      .finally(finalize);

    imuLoading = true;
    imuResource.refresh()
      .then((status) => {
        sawSuccess = true;
        loadError = null;
        imuLoading = false;
        applyImuStatus(status);
      })
      .catch((error) => {
        failures += 1;
        imuLoading = false;
        if (connectionStatus === 'online') {
          reportError({ context: 'Systems IMU refresh', error, toast: false });
        }
        const message = formatLoadError(error);
        imuError = message;
        systems = { ...systems, errors: { ...(systems.errors ?? {}), imu: message } };
        imuResource.invalidate();
      })
      .finally(finalize);
  }

  function selectActivityTab(tab: ActivityTabId): void {
    activeActivityTab = tab;
  }

  function handleFusionChange(value: string) {
    imuFusionChoice = value;
    syncImuDirtyFlag();
  }

  function handleRangeChange(value: string) {
    imuRangeChoice = value;
    syncImuDirtyFlag();
  }

  function handleIntervalChange(value: number) {
    imuIntervalChoice = value;
    syncImuDirtyFlag();
  }

  function handleDrVelocityDampTauChange(value: number) {
    imuDrVelocityDampTauChoice = value;
    syncImuDirtyFlag();
  }

  function handleDrStillVelocityZeroTauChange(value: number) {
    imuDrStillVelocityZeroTauChoice = value;
    syncImuDirtyFlag();
  }

  function handleDrMaxAccelWorldChange(value: number) {
    imuDrMaxAccelWorldChoice = value;
    syncImuDirtyFlag();
  }

  function handleDrMaxSpeedChange(value: number) {
    imuDrMaxSpeedChoice = value;
    syncImuDirtyFlag();
  }

  function handleDrMaxPositionChange(value: number) {
    imuDrMaxPositionChoice = value;
    syncImuDirtyFlag();
  }

  function handleDrLockPositionChange(value: boolean) {
    imuDrLockPositionChoice = value;
    syncImuDirtyFlag();
  }

  function resetImuForm() {
    imuFusionChoice = imu.fusion;
    imuRangeChoice = imu.range;
    imuIntervalChoice = imu.updateIntervalMs && imu.updateIntervalMs > 0 ? imu.updateIntervalMs : imuIntervalOptions[0] ?? 100;
    imuDrVelocityDampTauChoice = imu.drVelocityDampTauSeconds;
    imuDrStillVelocityZeroTauChoice = imu.drStillVelocityZeroTauSeconds;
    imuDrMaxAccelWorldChoice = imu.drMaxAccelWorldMps2;
    imuDrMaxSpeedChoice = imu.drMaxSpeedMps;
    imuDrMaxPositionChoice = imu.drMaxPositionM;
    imuDrLockPositionChoice = imu.drLockPosition;
    imuFormDirty = false;
  }

  function buildImuConfigPayload(): {
    fusion?: string;
    range?: string;
    updateIntervalMs?: number;
    drVelocityDampTauSeconds?: number;
    drStillVelocityZeroTauSeconds?: number;
    drMaxAccelWorldMps2?: number;
    drMaxSpeedMps?: number;
    drMaxPositionM?: number;
    drLockPosition?: boolean;
  } {
    const payload: {
      fusion?: string;
      range?: string;
      updateIntervalMs?: number;
      drVelocityDampTauSeconds?: number;
      drStillVelocityZeroTauSeconds?: number;
      drMaxAccelWorldMps2?: number;
      drMaxSpeedMps?: number;
      drMaxPositionM?: number;
      drLockPosition?: boolean;
    } = {};
    if (imuFusionChoice && imuFusionChoice !== imu.fusion) {
      payload.fusion = imuFusionChoice;
    }
    if (imuRangeChoice && imuRangeChoice !== imu.range) {
      payload.range = imuRangeChoice;
    }
    if (Number.isFinite(imuIntervalChoice) && imuIntervalChoice > 0 && imuIntervalChoice !== imu.updateIntervalMs) {
      payload.updateIntervalMs = imuIntervalChoice;
    }
    if (
      Number.isFinite(imuDrVelocityDampTauChoice) &&
      imuDrVelocityDampTauChoice > 0 &&
      imuDrVelocityDampTauChoice !== imu.drVelocityDampTauSeconds
    ) {
      payload.drVelocityDampTauSeconds = imuDrVelocityDampTauChoice;
    }
    if (
      Number.isFinite(imuDrStillVelocityZeroTauChoice) &&
      imuDrStillVelocityZeroTauChoice > 0 &&
      imuDrStillVelocityZeroTauChoice !== imu.drStillVelocityZeroTauSeconds
    ) {
      payload.drStillVelocityZeroTauSeconds = imuDrStillVelocityZeroTauChoice;
    }
    if (
      Number.isFinite(imuDrMaxAccelWorldChoice) &&
      imuDrMaxAccelWorldChoice > 0 &&
      imuDrMaxAccelWorldChoice !== imu.drMaxAccelWorldMps2
    ) {
      payload.drMaxAccelWorldMps2 = imuDrMaxAccelWorldChoice;
    }
    if (Number.isFinite(imuDrMaxSpeedChoice) && imuDrMaxSpeedChoice > 0 && imuDrMaxSpeedChoice !== imu.drMaxSpeedMps) {
      payload.drMaxSpeedMps = imuDrMaxSpeedChoice;
    }
    if (
      Number.isFinite(imuDrMaxPositionChoice) &&
      imuDrMaxPositionChoice > 0 &&
      imuDrMaxPositionChoice !== imu.drMaxPositionM
    ) {
      payload.drMaxPositionM = imuDrMaxPositionChoice;
    }
    if (imuDrLockPositionChoice !== imu.drLockPosition) {
      payload.drLockPosition = imuDrLockPositionChoice;
    }
    return payload;
  }

  function syncImuDirtyFlag() {
    imuFormDirty = Object.keys(buildImuConfigPayload()).length > 0;
  }

  async function rescanI2c(): Promise<void> {
    if (isRescanningI2c) return;
    i2cError = null;
    isRescanningI2c = true;
    try {
      const inventory = await refreshI2cInventory();
      const nextErrors = { ...(systems.errors ?? {}), i2c: null };
      systems = { ...systems, i2cInventory: inventory, errors: nextErrors, fetchedAt: Date.now() };
    } catch (error) {
      reportError({ context: 'I2C rescan', error, toast: false });
      const message = formatLoadError(error);
      i2cError = message;
      systems = { ...systems, errors: { ...(systems.errors ?? {}), i2c: message } };
    } finally {
      isRescanningI2c = false;
    }
  }

  function recordImuSample(sample: ImuStatus): void {
    if (!sample?.hasSample) return;
    const stamp = sample.updatedAt ? Date.parse(sample.updatedAt) : Date.now();
    const t = Number.isFinite(stamp) ? stamp : Date.now();
    if (lastImuTimestamp && t === lastImuTimestamp) return;
    lastImuTimestamp = t;
    const next = {
      t,
      accel: sample.accel,
      gyro: sample.gyro,
      mag: sample.mag,
      orientation: sample.orientation
    };
    imuHistory = [...imuHistory.slice(-(IMU_HISTORY_LIMIT - 1)), next];
  }

  function applyImuStatus(status: ImuStatus): void {
    const message = status.lastError ? formatLoadError(status.lastError) : null;
    const nextErrors = { ...(systems.errors ?? {}), imu: message };
    systems = { ...systems, imu: status, errors: nextErrors, fetchedAt: Date.now() };
    imuError = message;
    recordImuSample(status);
  }

  function handleImuStatusPush(status: ImuStatus): void {
    if (imuPollingPaused) return;
    imuStreamLastMessageAt = Date.now();
    applyImuStatus(status);
  }

  function currentImuStreamStaleMs(): number {
    const intervalMs =
      typeof imu.updateIntervalMs === 'number' && Number.isFinite(imu.updateIntervalMs) && imu.updateIntervalMs > 0
        ? imu.updateIntervalMs
        : IMU_POLL_MS;
    const dynamic = intervalMs * 4;
    return Math.max(IMU_STREAM_STALE_BASE_MS, Math.min(IMU_STREAM_STALE_MAX_MS, dynamic));
  }

  async function refreshImu(): Promise<void> {
    if (imuPollingPaused) return;
    if (isApplyingImuConfig) return;
    if (isBackendUnavailable) return;
    if (isRefreshingImu) return;
    isRefreshingImu = true;
    try {
      const status = await refreshImuStatus();
      applyImuStatus(status);
    } catch (error) {
      if (connectionStatus === 'online') {
        reportError({ context: 'IMU status refresh', error, toast: false });
      }
      const message = formatLoadError(error);
      imuError = message;
      systems = { ...systems, errors: { ...(systems.errors ?? {}), imu: message } };
    } finally {
      isRefreshingImu = false;
    }
  }

  async function applyImuConfig(request: {
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
    resetPose?: boolean;
  }): Promise<void> {
    if (isApplyingImuConfig) return;
    imuError = null;
    isApplyingImuConfig = true;
    imuPollingPaused = true;
    clearImuStreamReconnect();
    clearImuPollingFallback();
    stopImuStream();
    stopImuPolling();
    const touchesRuntimeConfig = Boolean(
      request.fusion ||
        request.range ||
        typeof request.updateIntervalMs === 'number' ||
        typeof request.drVelocityDampTauSeconds === 'number' ||
        typeof request.drStillVelocityZeroTauSeconds === 'number' ||
        typeof request.drMaxAccelWorldMps2 === 'number' ||
        typeof request.drMaxSpeedMps === 'number' ||
        typeof request.drMaxPositionM === 'number' ||
        typeof request.drLockPosition === 'boolean'
    );
    try {
      const status = await updateImuConfig(request);
      applyImuStatus(status);
      if (touchesRuntimeConfig) {
        imuFusionChoice = status.fusion;
        imuRangeChoice = status.range;
        imuIntervalChoice = status.updateIntervalMs;
        imuDrVelocityDampTauChoice = status.drVelocityDampTauSeconds;
        imuDrStillVelocityZeroTauChoice = status.drStillVelocityZeroTauSeconds;
        imuDrMaxAccelWorldChoice = status.drMaxAccelWorldMps2;
        imuDrMaxSpeedChoice = status.drMaxSpeedMps;
        imuDrMaxPositionChoice = status.drMaxPositionM;
        imuDrLockPositionChoice = status.drLockPosition;
        imuFormDirty = false;
      }
    } catch (error) {
      reportError({ context: 'Update IMU config', error, toast: false });
      const message = formatLoadError(error);
      imuError = message;
      systems = { ...systems, errors: { ...(systems.errors ?? {}), imu: message } };
    } finally {
      isApplyingImuConfig = false;
      imuPollingPaused = false;
      if (activeActivityTab === 'imu' && !imuStreaming) {
        startImuStream();
        scheduleImuPollingFallback();
        void refreshImu();
      }
    }
  }

  async function submitImuConfig(): Promise<void> {
    const payload = buildImuConfigPayload();
    if (!Object.keys(payload).length) {
      imuFormDirty = false;
      return;
    }
    await applyImuConfig(payload);
  }

  async function resetImuPose(): Promise<void> {
    await applyImuConfig({ resetPose: true });
  }

  function startImuStream(): void {
    if (isApplyingImuConfig) return;
    if (imuStreamClose || imuStreaming || activeActivityTab !== 'imu') return;
    if (Date.now() < imuStreamCooldownUntil) return;
    stopImuPolling();
    clearImuStreamReconnect();
    const teardown = connectImuStream({
      onOpen: () => {
        imuStreaming = true;
        imuStreamDisconnecting = false;
        imuStreamLastMessageAt = Date.now();
        stopImuPolling();
        clearImuPollingFallback();
      },
      onStatus: handleImuStatusPush,
      onError: (message) => {
        if (message) {
          imuError = formatLoadError(message);
        }
        handleImuStreamDisconnect();
      },
      onClose: () => handleImuStreamDisconnect()
    });
    imuStreamClose = () => {
      teardown();
      imuStreamClose = null;
    };
  }

  function forceImuStreamReconnect(): void {
    if (imuStreamDisconnecting) return;
    imuStreamCooldownUntil = Date.now() + 300;
    stopImuStream({ clearReconnect: true });
    if (activeActivityTab === 'imu') {
      startImuStream();
      if (!imuStreaming) startImuPolling();
    }
  }

  function handleImuStreamDisconnect(): void {
    if (imuStreamDisconnecting) return;
    imuStreamDisconnecting = true;
    stopImuStream({ clearReconnect: false });
    if (activeActivityTab === 'imu' && !isApplyingImuConfig) {
      scheduleImuPollingFallback();
      scheduleImuStreamReconnect();
    } else {
      clearImuStreamReconnect();
    }
    imuStreamDisconnecting = false;
  }

  function scheduleImuStreamReconnect(): void {
    if (isApplyingImuConfig) return;
    if (imuStreamReconnectTimer || activeActivityTab !== 'imu') return;
    imuStreamReconnectTimer = setTimeout(() => {
      imuStreamReconnectTimer = null;
      if (!imuStreaming && activeActivityTab === 'imu' && !isApplyingImuConfig) {
        startImuStream();
      }
    }, IMU_STREAM_RECONNECT_MS);
  }

  function clearImuStreamReconnect(): void {
    if (imuStreamReconnectTimer) {
      clearTimeout(imuStreamReconnectTimer);
      imuStreamReconnectTimer = null;
    }
  }

  function stopImuStream(options: { clearReconnect?: boolean } = {}): void {
    const { clearReconnect = true } = options;
    if (imuStreamClose) {
      imuStreamClose();
      imuStreamClose = null;
    }
    imuStreaming = false;
    imuStreamLastMessageAt = null;
    if (clearReconnect) {
      clearImuStreamReconnect();
    }
  }

  function startImuPolling(): void {
    if (isApplyingImuConfig) return;
    if (imuStreaming) return;
    if (activeActivityTab !== 'imu') return;
    if (stopImuPollLoop) return;
    if (!imuPollingPaused) {
      void refreshImu();
    }
    stopImuPollLoop = startRefreshScheduler(() => {
      if (!imuPollingPaused && !imuStreaming) {
        void refreshImu();
      }
    }, {
      intervalMs: IMU_POLL_MS,
      immediate: false,
      enabled: () => activeActivityTab === 'imu' && !imuStreaming && !isApplyingImuConfig
    });
  }

  function scheduleImuPollingFallback(): void {
    if (isApplyingImuConfig) return;
    if (imuStreaming) return;
    if (activeActivityTab !== 'imu') return;
    if (stopImuPollLoop) return;
    if (imuPollStartTimer) return;
    imuPollStartTimer = setTimeout(() => {
      imuPollStartTimer = null;
      if (!imuStreaming && activeActivityTab === 'imu' && !isApplyingImuConfig) {
        startImuPolling();
      }
    }, 1500);
  }

  function clearImuPollingFallback(): void {
    if (imuPollStartTimer) {
      clearTimeout(imuPollStartTimer);
      imuPollStartTimer = null;
    }
  }

  function stopImuPolling(): void {
    stopImuPollLoop?.();
    stopImuPollLoop = null;
    clearImuPollingFallback();
    clearImuFocusTimer();
    imuPollingPaused = false;
  }

  function clearImuFocusTimer(): void {
    if (imuFocusTimer) {
      clearTimeout(imuFocusTimer);
      imuFocusTimer = null;
    }
  }

  function handleImuFormFocus(): void {
    clearImuFocusTimer();
    imuPollingPaused = true;
  }

  function handleImuFormBlur(): void {
    clearImuFocusTimer();
    imuFocusTimer = setTimeout(() => {
      imuPollingPaused = false;
      imuFocusTimer = null;
      if (!imuStreaming) {
        void refreshImu();
      }
    }, 250);
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
          {#if activeActivityTab === 'logs'}
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
              {i2cError}
              {isRescanningI2c}
              {i2cBusOrder}
              {i2cInventory}
              onRescan={rescanI2c}
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
              onRefreshImu={refreshImu}
              onFusionChange={handleFusionChange}
              onRangeChange={handleRangeChange}
              onIntervalChange={handleIntervalChange}
              onDrVelocityDampTauChange={handleDrVelocityDampTauChange}
              onDrStillVelocityZeroTauChange={handleDrStillVelocityZeroTauChange}
              onDrMaxAccelWorldChange={handleDrMaxAccelWorldChange}
              onDrMaxSpeedChange={handleDrMaxSpeedChange}
              onDrMaxPositionChange={handleDrMaxPositionChange}
              onDrLockPositionChange={handleDrLockPositionChange}
              onApplyImuConfig={applyImuConfig}
              onResetImuPose={resetImuPose}
              onResetImuForm={resetImuForm}
              onSubmitImuConfig={submitImuConfig}
              onImuFormFocus={handleImuFormFocus}
              onImuFormBlur={handleImuFormBlur}
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
