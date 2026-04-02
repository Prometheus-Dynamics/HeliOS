import { startRefreshScheduler } from '$lib/api/refreshScheduler';
import { connectImuStream, refreshImuStatus, updateImuConfig } from '$lib/api/systemsPage';
import type { SensorOrientation } from '$lib/types/devices';
import type { ImuAxes, ImuStatus, SystemsPageData } from '$lib/types/systems';
import { reportError } from '$lib/ui/errorPolicy';
import { formatLoadError } from './systemsPageUtils';
import type { ActivityTabId } from './systemsPageTabs';

const IMU_POLL_MS = 1000;
const IMU_STREAM_RECONNECT_MS = 500;
const IMU_STREAM_STALE_BASE_MS = 750;
const IMU_STREAM_STALE_MAX_MS = 8000;
const IMU_HISTORY_LIMIT = 180;

type ImuHistoryEntry = {
  t: number;
  accel: ImuAxes;
  gyro: ImuAxes;
  mag: ImuAxes | null;
  orientation: SensorOrientation;
};

type ImuConfigPayload = {
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
};

type SystemsPageImuControllerOptions = {
  readSystems: () => SystemsPageData;
  setSystems: (value: SystemsPageData) => void;
  readImu: () => ImuStatus;
  readTabErrors: () => SystemsPageData['errors'];
  readActiveActivityTab: () => ActivityTabId;
  readConnectionStatus: () => string;
  readIsBackendUnavailable: () => boolean;
  readImuIntervalOptions: () => number[];
  readIsRefreshingImu: () => boolean;
  setIsRefreshingImu: (value: boolean) => void;
  readIsApplyingImuConfig: () => boolean;
  setIsApplyingImuConfig: (value: boolean) => void;
  readImuHistory: () => ImuHistoryEntry[];
  setImuHistory: (value: ImuHistoryEntry[]) => void;
  readLastImuTimestamp: () => number | null;
  setLastImuTimestamp: (value: number | null) => void;
  readImuStreaming: () => boolean;
  setImuStreaming: (value: boolean) => void;
  readImuStreamLastMessageAt: () => number | null;
  setImuStreamLastMessageAt: (value: number | null) => void;
  readImuStreamCooldownUntil: () => number;
  setImuStreamCooldownUntil: (value: number) => void;
  readImuPollingPaused: () => boolean;
  setImuPollingPaused: (value: boolean) => void;
  readImuError: () => string | null;
  setImuError: (value: string | null) => void;
  readImuFusionChoice: () => string;
  setImuFusionChoice: (value: string) => void;
  readImuRangeChoice: () => string;
  setImuRangeChoice: (value: string) => void;
  readImuIntervalChoice: () => number;
  setImuIntervalChoice: (value: number) => void;
  readImuDrVelocityDampTauChoice: () => number;
  setImuDrVelocityDampTauChoice: (value: number) => void;
  readImuDrStillVelocityZeroTauChoice: () => number;
  setImuDrStillVelocityZeroTauChoice: (value: number) => void;
  readImuDrMaxAccelWorldChoice: () => number;
  setImuDrMaxAccelWorldChoice: (value: number) => void;
  readImuDrMaxSpeedChoice: () => number;
  setImuDrMaxSpeedChoice: (value: number) => void;
  readImuDrMaxPositionChoice: () => number;
  setImuDrMaxPositionChoice: (value: number) => void;
  readImuDrLockPositionChoice: () => boolean;
  setImuDrLockPositionChoice: (value: boolean) => void;
  readImuFormDirty: () => boolean;
  setImuFormDirty: (value: boolean) => void;
};

export const createSystemsPageImuController = (options: SystemsPageImuControllerOptions) => {
  let stopImuPollLoop: (() => void) | null = null;
  let imuPollStartTimer: ReturnType<typeof setTimeout> | null = null;
  let imuStreamClose: (() => void) | null = null;
  let imuStreamReconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let imuStreamDisconnecting = false;
  let imuFocusTimer: ReturnType<typeof setTimeout> | null = null;
  let stopImuWatchdog: (() => void) | null = null;

  const buildImuConfigPayload = (): ImuConfigPayload => {
    const imu = options.readImu();
    const payload: ImuConfigPayload = {};
    const intervalOptions = options.readImuIntervalOptions();

    if (options.readImuFusionChoice() && options.readImuFusionChoice() !== imu.fusion) {
      payload.fusion = options.readImuFusionChoice();
    }
    if (options.readImuRangeChoice() && options.readImuRangeChoice() !== imu.range) {
      payload.range = options.readImuRangeChoice();
    }
    if (
      Number.isFinite(options.readImuIntervalChoice()) &&
      options.readImuIntervalChoice() > 0 &&
      options.readImuIntervalChoice() !== imu.updateIntervalMs
    ) {
      payload.updateIntervalMs = options.readImuIntervalChoice();
    }
    if (
      Number.isFinite(options.readImuDrVelocityDampTauChoice()) &&
      options.readImuDrVelocityDampTauChoice() > 0 &&
      options.readImuDrVelocityDampTauChoice() !== imu.drVelocityDampTauSeconds
    ) {
      payload.drVelocityDampTauSeconds = options.readImuDrVelocityDampTauChoice();
    }
    if (
      Number.isFinite(options.readImuDrStillVelocityZeroTauChoice()) &&
      options.readImuDrStillVelocityZeroTauChoice() > 0 &&
      options.readImuDrStillVelocityZeroTauChoice() !== imu.drStillVelocityZeroTauSeconds
    ) {
      payload.drStillVelocityZeroTauSeconds = options.readImuDrStillVelocityZeroTauChoice();
    }
    if (
      Number.isFinite(options.readImuDrMaxAccelWorldChoice()) &&
      options.readImuDrMaxAccelWorldChoice() > 0 &&
      options.readImuDrMaxAccelWorldChoice() !== imu.drMaxAccelWorldMps2
    ) {
      payload.drMaxAccelWorldMps2 = options.readImuDrMaxAccelWorldChoice();
    }
    if (
      Number.isFinite(options.readImuDrMaxSpeedChoice()) &&
      options.readImuDrMaxSpeedChoice() > 0 &&
      options.readImuDrMaxSpeedChoice() !== imu.drMaxSpeedMps
    ) {
      payload.drMaxSpeedMps = options.readImuDrMaxSpeedChoice();
    }
    if (
      Number.isFinite(options.readImuDrMaxPositionChoice()) &&
      options.readImuDrMaxPositionChoice() > 0 &&
      options.readImuDrMaxPositionChoice() !== imu.drMaxPositionM
    ) {
      payload.drMaxPositionM = options.readImuDrMaxPositionChoice();
    }
    if (options.readImuDrLockPositionChoice() !== imu.drLockPosition) {
      payload.drLockPosition = options.readImuDrLockPositionChoice();
    }

    if (
      !Number.isFinite(options.readImuIntervalChoice()) ||
      options.readImuIntervalChoice() <= 0
    ) {
      const fallbackInterval = intervalOptions[0] ?? 100;
      if (fallbackInterval !== imu.updateIntervalMs) {
        payload.updateIntervalMs = fallbackInterval;
      }
    }

    return payload;
  };

  const syncImuDirtyFlag = (): void => {
    options.setImuFormDirty(Object.keys(buildImuConfigPayload()).length > 0);
  };

  const handleFusionChange = (value: string): void => {
    options.setImuFusionChoice(value);
    syncImuDirtyFlag();
  };

  const handleRangeChange = (value: string): void => {
    options.setImuRangeChoice(value);
    syncImuDirtyFlag();
  };

  const handleIntervalChange = (value: number): void => {
    options.setImuIntervalChoice(value);
    syncImuDirtyFlag();
  };

  const handleDrVelocityDampTauChange = (value: number): void => {
    options.setImuDrVelocityDampTauChoice(value);
    syncImuDirtyFlag();
  };

  const handleDrStillVelocityZeroTauChange = (value: number): void => {
    options.setImuDrStillVelocityZeroTauChoice(value);
    syncImuDirtyFlag();
  };

  const handleDrMaxAccelWorldChange = (value: number): void => {
    options.setImuDrMaxAccelWorldChoice(value);
    syncImuDirtyFlag();
  };

  const handleDrMaxSpeedChange = (value: number): void => {
    options.setImuDrMaxSpeedChoice(value);
    syncImuDirtyFlag();
  };

  const handleDrMaxPositionChange = (value: number): void => {
    options.setImuDrMaxPositionChoice(value);
    syncImuDirtyFlag();
  };

  const handleDrLockPositionChange = (value: boolean): void => {
    options.setImuDrLockPositionChoice(value);
    syncImuDirtyFlag();
  };

  const resetImuForm = (): void => {
    const imu = options.readImu();
    const intervalOptions = options.readImuIntervalOptions();
    options.setImuFusionChoice(imu.fusion);
    options.setImuRangeChoice(imu.range);
    options.setImuIntervalChoice(
      imu.updateIntervalMs && imu.updateIntervalMs > 0
        ? imu.updateIntervalMs
        : intervalOptions[0] ?? 100
    );
    options.setImuDrVelocityDampTauChoice(imu.drVelocityDampTauSeconds);
    options.setImuDrStillVelocityZeroTauChoice(imu.drStillVelocityZeroTauSeconds);
    options.setImuDrMaxAccelWorldChoice(imu.drMaxAccelWorldMps2);
    options.setImuDrMaxSpeedChoice(imu.drMaxSpeedMps);
    options.setImuDrMaxPositionChoice(imu.drMaxPositionM);
    options.setImuDrLockPositionChoice(imu.drLockPosition);
    options.setImuFormDirty(false);
  };

  const recordImuSample = (sample: ImuStatus): void => {
    if (!sample?.hasSample) return;
    const stamp = sample.updatedAt ? Date.parse(sample.updatedAt) : Date.now();
    const t = Number.isFinite(stamp) ? stamp : Date.now();
    if (options.readLastImuTimestamp() && t === options.readLastImuTimestamp()) return;
    options.setLastImuTimestamp(t);
    const next: ImuHistoryEntry = {
      t,
      accel: sample.accel,
      gyro: sample.gyro,
      mag: sample.mag,
      orientation: sample.orientation
    };
    options.setImuHistory([
      ...options.readImuHistory().slice(-(IMU_HISTORY_LIMIT - 1)),
      next
    ]);
  };

  const applyImuStatus = (status: ImuStatus): void => {
    const message = status.lastError ? formatLoadError(status.lastError) : null;
    options.setSystems({
      ...options.readSystems(),
      imu: status,
      errors: { ...(options.readSystems().errors ?? {}), imu: message },
      fetchedAt: Date.now()
    });
    options.setImuError(message);
    recordImuSample(status);
    if (!options.readImuFormDirty()) {
      const intervalOptions = options.readImuIntervalOptions();
      options.setImuFusionChoice(status.fusion);
      options.setImuRangeChoice(status.range);
      options.setImuIntervalChoice(
        status.updateIntervalMs && status.updateIntervalMs > 0
          ? status.updateIntervalMs
          : intervalOptions[0] ?? 100
      );
      options.setImuDrVelocityDampTauChoice(status.drVelocityDampTauSeconds);
      options.setImuDrStillVelocityZeroTauChoice(status.drStillVelocityZeroTauSeconds);
      options.setImuDrMaxAccelWorldChoice(status.drMaxAccelWorldMps2);
      options.setImuDrMaxSpeedChoice(status.drMaxSpeedMps);
      options.setImuDrMaxPositionChoice(status.drMaxPositionM);
      options.setImuDrLockPositionChoice(status.drLockPosition);
    }
  };

  const handleImuStatusPush = (status: ImuStatus): void => {
    if (options.readImuPollingPaused()) return;
    options.setImuStreamLastMessageAt(Date.now());
    applyImuStatus(status);
  };

  const currentImuStreamStaleMs = (): number => {
    const imu = options.readImu();
    const intervalMs =
      typeof imu.updateIntervalMs === 'number' &&
      Number.isFinite(imu.updateIntervalMs) &&
      imu.updateIntervalMs > 0
        ? imu.updateIntervalMs
        : IMU_POLL_MS;
    const dynamic = intervalMs * 4;
    return Math.max(IMU_STREAM_STALE_BASE_MS, Math.min(IMU_STREAM_STALE_MAX_MS, dynamic));
  };

  const refreshImu = async (): Promise<void> => {
    if (options.readImuPollingPaused()) return;
    if (options.readIsApplyingImuConfig()) return;
    if (options.readIsBackendUnavailable()) return;
    if (options.readIsRefreshingImu()) return;
    options.setIsRefreshingImu(true);
    try {
      applyImuStatus(await refreshImuStatus());
    } catch (error) {
      if (options.readConnectionStatus() === 'online') {
        reportError({ context: 'IMU status refresh', error, toast: false });
      }
      const message = formatLoadError(error);
      options.setImuError(message);
      options.setSystems({
        ...options.readSystems(),
        errors: { ...(options.readSystems().errors ?? {}), imu: message }
      });
    } finally {
      options.setIsRefreshingImu(false);
    }
  };

  const clearImuStreamReconnect = (): void => {
    if (imuStreamReconnectTimer) {
      clearTimeout(imuStreamReconnectTimer);
      imuStreamReconnectTimer = null;
    }
  };

  const clearImuPollingFallback = (): void => {
    if (imuPollStartTimer) {
      clearTimeout(imuPollStartTimer);
      imuPollStartTimer = null;
    }
  };

  const clearImuFocusTimer = (): void => {
    if (imuFocusTimer) {
      clearTimeout(imuFocusTimer);
      imuFocusTimer = null;
    }
  };

  const stopImuPolling = (): void => {
    stopImuPollLoop?.();
    stopImuPollLoop = null;
    clearImuPollingFallback();
    clearImuFocusTimer();
    options.setImuPollingPaused(false);
  };

  const stopImuWatchdogLoop = (): void => {
    stopImuWatchdog?.();
    stopImuWatchdog = null;
  };

  const stopImuStream = (runtimeOptions: { clearReconnect?: boolean } = {}): void => {
    const { clearReconnect = true } = runtimeOptions;
    if (imuStreamClose) {
      imuStreamClose();
      imuStreamClose = null;
    }
    options.setImuStreaming(false);
    options.setImuStreamLastMessageAt(null);
    if (clearReconnect) {
      clearImuStreamReconnect();
    }
    stopImuWatchdogLoop();
  };

  const ensureImuWatchdog = (): void => {
    if (stopImuWatchdog) return;
    stopImuWatchdog = startRefreshScheduler(() => {
      if (options.readActiveActivityTab() !== 'imu') return;
      if (!options.readImuStreaming()) return;
      if (options.readImuPollingPaused()) return;
      if (options.readIsApplyingImuConfig()) return;
      const last = options.readImuStreamLastMessageAt();
      if (last == null) return;
      if (Date.now() - last <= currentImuStreamStaleMs()) return;
      forceImuStreamReconnect();
    }, {
      intervalMs: 250,
      immediate: false,
      enabled: () =>
        options.readActiveActivityTab() === 'imu' &&
        options.readImuStreaming() &&
        !options.readImuPollingPaused() &&
        !options.readIsApplyingImuConfig()
    });
  };

  const startImuPolling = (): void => {
    if (options.readIsApplyingImuConfig()) return;
    if (options.readImuStreaming()) return;
    if (options.readActiveActivityTab() !== 'imu') return;
    if (stopImuPollLoop) return;
    if (!options.readImuPollingPaused()) {
      void refreshImu();
    }
    stopImuPollLoop = startRefreshScheduler(() => {
      if (!options.readImuPollingPaused() && !options.readImuStreaming()) {
        void refreshImu();
      }
    }, {
      intervalMs: IMU_POLL_MS,
      immediate: false,
      enabled: () =>
        options.readActiveActivityTab() === 'imu' &&
        !options.readImuStreaming() &&
        !options.readIsApplyingImuConfig()
    });
  };

  const scheduleImuPollingFallback = (): void => {
    if (options.readIsApplyingImuConfig()) return;
    if (options.readImuStreaming()) return;
    if (options.readActiveActivityTab() !== 'imu') return;
    if (stopImuPollLoop) return;
    if (imuPollStartTimer) return;
    imuPollStartTimer = setTimeout(() => {
      imuPollStartTimer = null;
      if (
        !options.readImuStreaming() &&
        options.readActiveActivityTab() === 'imu' &&
        !options.readIsApplyingImuConfig()
      ) {
        startImuPolling();
      }
    }, 1500);
  };

  const scheduleImuStreamReconnect = (): void => {
    if (options.readIsApplyingImuConfig()) return;
    if (imuStreamReconnectTimer || options.readActiveActivityTab() !== 'imu') return;
    imuStreamReconnectTimer = setTimeout(() => {
      imuStreamReconnectTimer = null;
      if (
        !options.readImuStreaming() &&
        options.readActiveActivityTab() === 'imu' &&
        !options.readIsApplyingImuConfig()
      ) {
        startImuStream();
      }
    }, IMU_STREAM_RECONNECT_MS);
  };

  const handleImuStreamDisconnect = (): void => {
    if (imuStreamDisconnecting) return;
    imuStreamDisconnecting = true;
    stopImuStream({ clearReconnect: false });
    if (options.readActiveActivityTab() === 'imu' && !options.readIsApplyingImuConfig()) {
      scheduleImuPollingFallback();
      scheduleImuStreamReconnect();
    } else {
      clearImuStreamReconnect();
    }
    imuStreamDisconnecting = false;
  };

  const startImuStream = (): void => {
    if (options.readIsApplyingImuConfig()) return;
    if (imuStreamClose || options.readImuStreaming() || options.readActiveActivityTab() !== 'imu') return;
    if (Date.now() < options.readImuStreamCooldownUntil()) return;
    stopImuPolling();
    clearImuStreamReconnect();
    const teardown = connectImuStream({
      onOpen: () => {
        options.setImuStreaming(true);
        imuStreamDisconnecting = false;
        options.setImuStreamLastMessageAt(Date.now());
        stopImuPolling();
        clearImuPollingFallback();
        ensureImuWatchdog();
      },
      onStatus: handleImuStatusPush,
      onError: (message) => {
        if (message) {
          options.setImuError(formatLoadError(message));
        }
        handleImuStreamDisconnect();
      },
      onClose: () => handleImuStreamDisconnect()
    });
    imuStreamClose = () => {
      teardown();
      imuStreamClose = null;
    };
    ensureImuWatchdog();
  };

  const forceImuStreamReconnect = (): void => {
    if (imuStreamDisconnecting) return;
    options.setImuStreamCooldownUntil(Date.now() + 300);
    stopImuStream({ clearReconnect: true });
    if (options.readActiveActivityTab() === 'imu') {
      startImuStream();
      if (!options.readImuStreaming()) startImuPolling();
    }
  };

  const applyImuConfig = async (request: ImuConfigPayload): Promise<void> => {
    if (options.readIsApplyingImuConfig()) return;
    options.setImuError(null);
    options.setIsApplyingImuConfig(true);
    options.setImuPollingPaused(true);
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
        options.setImuFusionChoice(status.fusion);
        options.setImuRangeChoice(status.range);
        options.setImuIntervalChoice(status.updateIntervalMs);
        options.setImuDrVelocityDampTauChoice(status.drVelocityDampTauSeconds);
        options.setImuDrStillVelocityZeroTauChoice(status.drStillVelocityZeroTauSeconds);
        options.setImuDrMaxAccelWorldChoice(status.drMaxAccelWorldMps2);
        options.setImuDrMaxSpeedChoice(status.drMaxSpeedMps);
        options.setImuDrMaxPositionChoice(status.drMaxPositionM);
        options.setImuDrLockPositionChoice(status.drLockPosition);
        options.setImuFormDirty(false);
      }
    } catch (error) {
      reportError({ context: 'Update IMU config', error, toast: false });
      const message = formatLoadError(error);
      options.setImuError(message);
      options.setSystems({
        ...options.readSystems(),
        errors: { ...(options.readSystems().errors ?? {}), imu: message }
      });
    } finally {
      options.setIsApplyingImuConfig(false);
      options.setImuPollingPaused(false);
      if (options.readActiveActivityTab() === 'imu' && !options.readImuStreaming()) {
        startImuStream();
        scheduleImuPollingFallback();
        void refreshImu();
      }
    }
  };

  const submitImuConfig = async (): Promise<void> => {
    const payload = buildImuConfigPayload();
    if (!Object.keys(payload).length) {
      options.setImuFormDirty(false);
      return;
    }
    await applyImuConfig(payload);
  };

  const resetImuPose = async (): Promise<void> => {
    await applyImuConfig({ resetPose: true });
  };

  const handleImuFormFocus = (): void => {
    clearImuFocusTimer();
    options.setImuPollingPaused(true);
  };

  const handleImuFormBlur = (): void => {
    clearImuFocusTimer();
    imuFocusTimer = setTimeout(() => {
      options.setImuPollingPaused(false);
      imuFocusTimer = null;
      if (!options.readImuStreaming()) {
        void refreshImu();
      }
    }, 250);
  };

  const syncActiveTabRuntime = (): void => {
    if (options.readActiveActivityTab() === 'imu') {
      options.setImuPollingPaused(false);
      startImuStream();
      scheduleImuPollingFallback();
      ensureImuWatchdog();
    } else {
      stopImuStream();
      stopImuPolling();
      stopImuWatchdogLoop();
    }
  };

  const syncObservedImuState = (): void => {
    const imu = options.readImu();
    const tabErrors = options.readTabErrors();
    if (imu.lastError) {
      options.setImuError(formatLoadError(imu.lastError));
    } else {
      options.setImuError(tabErrors.imu ?? null);
    }
    recordImuSample(imu);
  };

  const syncImuFormDefaults = (): void => {
    if (options.readImuFormDirty()) return;
    const imu = options.readImu();
    const intervalOptions = options.readImuIntervalOptions();
    const interval =
      imu.updateIntervalMs && imu.updateIntervalMs > 0
        ? imu.updateIntervalMs
        : intervalOptions[0] ?? 100;
    options.setImuFusionChoice(imu.fusion);
    options.setImuRangeChoice(imu.range);
    options.setImuIntervalChoice(interval);
    options.setImuDrVelocityDampTauChoice(imu.drVelocityDampTauSeconds);
    options.setImuDrStillVelocityZeroTauChoice(imu.drStillVelocityZeroTauSeconds);
    options.setImuDrMaxAccelWorldChoice(imu.drMaxAccelWorldMps2);
    options.setImuDrMaxSpeedChoice(imu.drMaxSpeedMps);
    options.setImuDrMaxPositionChoice(imu.drMaxPositionM);
    options.setImuDrLockPositionChoice(imu.drLockPosition);
  };

  const destroy = (): void => {
    stopImuWatchdogLoop();
    stopImuStream();
    stopImuPolling();
  };

  return {
    applyImuStatus,
    syncActiveTabRuntime,
    syncObservedImuState,
    syncImuFormDefaults,
    handleFusionChange,
    handleRangeChange,
    handleIntervalChange,
    handleDrVelocityDampTauChange,
    handleDrStillVelocityZeroTauChange,
    handleDrMaxAccelWorldChange,
    handleDrMaxSpeedChange,
    handleDrMaxPositionChange,
    handleDrLockPositionChange,
    resetImuForm,
    buildImuConfigPayload,
    refreshImu,
    applyImuConfig,
    submitImuConfig,
    resetImuPose,
    handleImuFormFocus,
    handleImuFormBlur,
    destroy
  };
};
