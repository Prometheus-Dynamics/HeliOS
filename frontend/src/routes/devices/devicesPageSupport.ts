import type { DevicesPageData } from '$lib/types/devices';
import type { CameraRow } from '$lib/components/devices/DevicesCamerasPanel.svelte';
import { toaster } from '$lib/toaster';
import { createDomainResource, subscribeDomainResourceInvalidations } from '$lib/api/domainResources';
import { scheduleWhenIdle } from '$lib/utils/browserSchedule';
import { resourceGuardStatusResource, type ResourceGuardStatus } from '$lib/api/deviceStatusResources';
import { connectDevicesUpdatesStream } from '$lib/api/devicesUpdates';
import { startRefreshScheduler } from '$lib/api/refreshScheduler';
import { invalidateOwnedStreamMutationResources } from '$lib/api/streamResources';
import { StreamsApi } from '$lib/api/streamsApi';
import { apiFetch } from '$lib/api/core/http';
import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
import { createBackoffTimer } from '$lib/utils/backoff';
import { fetchDevicesCamerasSnapshot, fetchDevicesPeripheralsSnapshot } from '$lib/api/devices/fetchers';
import { localizationConfigResource, type LocalizationConfig } from '$lib/features/localization/localizationConfig';

const AUTO_REFRESH_MS = 10_000;
const DEVICES_CAMERAS_CACHE_KEY = 'devices:cameras:v1';
const DEVICES_PERIPHERALS_CACHE_KEY = 'devices:peripherals:v1';
const DEVICES_CACHE_STALE_MS = 10_000;
const DEVICES_CACHE_MAX_MS = 120_000;
const WS_REFRESH_DEBOUNCE_MS = 250;
const DEVICES_UPDATES_RECONNECT_MS = 750;
const DEVICES_UPDATES_RECONNECT_MAX_MS = 15_000;
const LOCALIZATION_CONFIG_REFRESH_MS = 30_000;

const devicesCamerasResource = createDomainResource({
  key: DEVICES_CAMERAS_CACHE_KEY,
  loader: fetchDevicesCamerasSnapshot,
  staleMs: DEVICES_CACHE_STALE_MS,
  maxAgeMs: DEVICES_CACHE_MAX_MS,
  kinds: ['device', 'settings', 'streams']
});

const devicesPeripheralsResource = createDomainResource({
  key: DEVICES_PERIPHERALS_CACHE_KEY,
  loader: fetchDevicesPeripheralsSnapshot,
  staleMs: DEVICES_CACHE_STALE_MS,
  maxAgeMs: DEVICES_CACHE_MAX_MS,
  kinds: ['device', 'settings']
});

export type PeripheralRow = DevicesPageData['peripherals'][number];

export type StreamActionBusyState = {
  unregister?: boolean;
  download?: boolean;
  restore?: boolean;
};

type DevicesPageSupportOptions = {
  readDevices: () => DevicesPageData;
  setDevices: (value: DevicesPageData) => void;
  readHasLoadedOnce: () => boolean;
  setHasLoadedOnce: (value: boolean) => void;
  readIsRefreshing: () => boolean;
  setIsRefreshing: (value: boolean) => void;
  setCamerasLoading: (value: boolean) => void;
  setPeripheralsLoading: (value: boolean) => void;
  setLocalizationConfig: (value: LocalizationConfig | null) => void;
  setResourceGuardStatus: (value: ResourceGuardStatus | null) => void;
  setLoadError: (value: string | null) => void;
  setFetchedAt: (value: number) => void;
  readConnectionStatus: () => string;
  readIsBackendUnavailable: () => boolean;
  syncActivePeripheral: (entries: PeripheralRow[]) => void;
  readStreamActionBusy: () => Record<string, StreamActionBusyState>;
  setStreamActionBusy: (value: Record<string, StreamActionBusyState>) => void;
  readPendingUnregister: () => CameraRow | null;
  setPendingUnregister: (value: CameraRow | null) => void;
};

const isAbortError = (error: unknown): boolean => {
  if (!error) return false;
  if (error instanceof DOMException && error.name === 'AbortError') return true;
  if (error instanceof Error && error.name === 'AbortError') return true;
  return (error as { name?: string }).name === 'AbortError';
};

export const createDevicesPageSupport = (options: DevicesPageSupportOptions) => {
  let stopRefreshScheduler: (() => void) | null = null;
  let stopResourceGuardScheduler: (() => void) | null = null;
  let stopLocalizationScheduler: (() => void) | null = null;
  let wsRefreshTimer: number | null = null;
  let updatesCleanup: (() => void) | null = null;
  let stopDomainInvalidation: (() => void) | null = null;
  let updatesNonce = 0;
  let updatesReconnectPending = false;
  let cancelBootstrapRefresh: (() => void) | null = null;

  const updatesReconnectBackoff = createBackoffTimer({
    baseMs: DEVICES_UPDATES_RECONNECT_MS,
    maxMs: DEVICES_UPDATES_RECONNECT_MAX_MS
  });

  const actionIsBusy = (cameraId: string, action: keyof StreamActionBusyState): boolean => {
    const entry = options.readStreamActionBusy()[cameraId];
    if (!entry) return false;
    return Boolean(entry[action]);
  };

  const setActionBusy = (
    cameraId: string,
    action: keyof StreamActionBusyState,
    value: boolean
  ): void => {
    const current = options.readStreamActionBusy();
    options.setStreamActionBusy({
      ...current,
      [cameraId]: {
        ...(current[cameraId] ?? {}),
        [action]: value
      }
    });
  };

  const refreshLocalizationConfig = async (): Promise<void> => {
    try {
      options.setLocalizationConfig(await localizationConfigResource.refresh());
    } catch (error) {
      if (options.readConnectionStatus() === 'online' && !isAbortError(error)) {
        reportError({ context: 'Devices localization refresh', error, toast: false });
      }
    }
  };

  const refreshResourceGuardStatus = async (): Promise<void> => {
    try {
      options.setResourceGuardStatus(await resourceGuardStatusResource.refresh());
    } catch {
      options.setResourceGuardStatus(null);
    }
  };

  const refreshDevices = async (
    runtimeOptions: { bootstrap?: boolean; force?: boolean } = {}
  ): Promise<void> => {
    if (options.readIsRefreshing()) return;

    const { bootstrap = false, force = false } = runtimeOptions;
    if (options.readIsBackendUnavailable()) {
      if (bootstrap && !options.readHasLoadedOnce()) {
        options.setLoadError('Backend unavailable. Update the API base URL in Settings.');
        options.setHasLoadedOnce(true);
      }
      return;
    }
    if (bootstrap) {
      options.setLoadError(null);
    }
    options.setIsRefreshing(true);
    let pending = 2;
    let failures = 0;
    let sawSuccess = false;

    const finalize = () => {
      pending -= 1;
      if (pending > 0) return;
      options.setIsRefreshing(false);
      if (bootstrap || !options.readHasLoadedOnce()) {
        options.setHasLoadedOnce(true);
      }
      if (!sawSuccess && failures >= 2) {
        options.setLoadError('Unable to load devices right now.');
      }
    };

    options.setCamerasLoading(true);
    devicesCamerasResource
      .refresh({ force })
      .then((camerasPayload) => {
        sawSuccess = true;
        options.setLoadError(null);
        options.setCamerasLoading(false);
        options.setDevices({ ...options.readDevices(), cameras: camerasPayload });
        options.setFetchedAt(Date.now());
      })
      .catch((error) => {
        failures += 1;
        options.setCamerasLoading(false);
        if (!isAbortError(error) && options.readConnectionStatus() === 'online') {
          reportError({ context: 'Devices cameras refresh', error, toast: false });
        }
        devicesCamerasResource.invalidate();
      })
      .finally(finalize);

    options.setPeripheralsLoading(true);
    devicesPeripheralsResource
      .refresh({ force })
      .then((snapshot) => {
        sawSuccess = true;
        options.setLoadError(null);
        options.setPeripheralsLoading(false);
        options.setDevices({
          ...options.readDevices(),
          peripherals: snapshot.peripherals,
          errors: { ...(options.readDevices().errors ?? {}), peripherals: snapshot.error }
        });
        options.syncActivePeripheral(snapshot.peripherals);
        options.setFetchedAt(Date.now());
      })
      .catch((error) => {
        failures += 1;
        options.setPeripheralsLoading(false);
        if (!isAbortError(error)) {
          if (options.readConnectionStatus() === 'online') {
            reportError({ context: 'Devices peripherals refresh', error, toast: false });
          }
          const message = buildErrorMessage({
            error,
            fallback: 'Unable to load peripherals right now.'
          });
          options.setDevices({
            ...options.readDevices(),
            errors: { ...(options.readDevices().errors ?? {}), peripherals: message }
          });
        }
        devicesPeripheralsResource.invalidate();
      })
      .finally(finalize);
  };

  const scheduleWsRefresh = (): void => {
    if (wsRefreshTimer != null) return;
    wsRefreshTimer = window.setTimeout(() => {
      wsRefreshTimer = null;
      if (document.hidden) return;
      void refreshDevices({ force: true });
    }, WS_REFRESH_DEBOUNCE_MS);
  };

  const scheduleDevicesUpdatesReconnect = (): void => {
    if (updatesReconnectPending) return;
    updatesReconnectPending = true;
    const delay = updatesReconnectBackoff.bump();
    updatesReconnectBackoff.schedule(() => {
      updatesReconnectPending = false;
      connectDevicesUpdates();
    }, delay);
  };

  const disconnectDevicesUpdates = (): void => {
    updatesNonce += 1;
    updatesReconnectPending = false;
    updatesReconnectBackoff.cancel();
    updatesCleanup?.();
    updatesCleanup = null;
  };

  const connectDevicesUpdates = (): void => {
    const nonce = (updatesNonce += 1);
    updatesReconnectPending = false;
    updatesReconnectBackoff.cancel();
    updatesCleanup?.();
    updatesCleanup = null;
    updatesCleanup = connectDevicesUpdatesStream({
      onOpen: () => {
        if (nonce !== updatesNonce) return;
        updatesReconnectBackoff.reset();
      },
      onUpdate: () => {
        if (nonce !== updatesNonce) return;
        updatesReconnectBackoff.reset();
        if (document.hidden) return;
        scheduleWsRefresh();
      },
      onClose: () => {
        if (nonce !== updatesNonce) return;
        scheduleDevicesUpdatesReconnect();
      },
      onError: (message) => {
        if (nonce !== updatesNonce) return;
        if (options.readConnectionStatus() === 'online') {
          reportError({ context: 'Devices updates socket', error: message, toast: false });
        }
        scheduleDevicesUpdatesReconnect();
      }
    });
  };

  const start = (): (() => void) => {
    const cachedCameras = devicesCamerasResource.read();
    if (cachedCameras?.data) {
      options.setDevices({ ...options.readDevices(), cameras: cachedCameras.data });
      options.setFetchedAt(cachedCameras.fetchedAt);
      options.setHasLoadedOnce(true);
    }

    const cachedPeripherals = devicesPeripheralsResource.read();
    if (cachedPeripherals?.data) {
      options.setDevices({
        ...options.readDevices(),
        peripherals: cachedPeripherals.data.peripherals,
        errors: {
          ...(options.readDevices().errors ?? {}),
          peripherals: cachedPeripherals.data.error
        }
      });
      options.setFetchedAt(cachedPeripherals.fetchedAt);
      options.setHasLoadedOnce(true);
    }

    const cachedLocalization = localizationConfigResource.read();
    if (cachedLocalization?.data) {
      options.setLocalizationConfig(cachedLocalization.data);
    }

    const cachedResourceGuard = resourceGuardStatusResource.read();
    if (cachedResourceGuard?.data) {
      options.setResourceGuardStatus(cachedResourceGuard.data);
    }

    if (options.readHasLoadedOnce()) {
      cancelBootstrapRefresh = scheduleWhenIdle(() => {
        if (document.hidden) return;
        void refreshDevices();
        void refreshLocalizationConfig();
        void refreshResourceGuardStatus();
      }, { timeoutMs: 1800, fallbackMs: 650 });
    } else {
      void refreshDevices({ bootstrap: true });
      void refreshLocalizationConfig();
      void refreshResourceGuardStatus();
    }

    connectDevicesUpdates();
    stopDomainInvalidation = subscribeDomainResourceInvalidations(
      ['device', 'localization', 'media', 'settings', 'streams'],
      [devicesCamerasResource, devicesPeripheralsResource],
      () => {
        void refreshDevices({ force: true });
        void refreshLocalizationConfig();
        void refreshResourceGuardStatus();
      },
      { debounceMs: WS_REFRESH_DEBOUNCE_MS }
    );
    stopRefreshScheduler = startRefreshScheduler(() => refreshDevices(), {
      intervalMs: AUTO_REFRESH_MS,
      immediate: false,
      enabled: () => !options.readIsBackendUnavailable()
    });
    stopResourceGuardScheduler = startRefreshScheduler(refreshResourceGuardStatus, {
      intervalMs: 4_000,
      immediate: false,
      enabled: () => !options.readIsBackendUnavailable()
    });
    stopLocalizationScheduler = startRefreshScheduler(refreshLocalizationConfig, {
      intervalMs: LOCALIZATION_CONFIG_REFRESH_MS,
      immediate: false,
      enabled: () => !options.readIsBackendUnavailable()
    });

    return stop;
  };

  const stop = (): void => {
    cancelBootstrapRefresh?.();
    cancelBootstrapRefresh = null;
    stopRefreshScheduler?.();
    stopRefreshScheduler = null;
    stopResourceGuardScheduler?.();
    stopResourceGuardScheduler = null;
    stopLocalizationScheduler?.();
    stopLocalizationScheduler = null;
    if (wsRefreshTimer) {
      clearTimeout(wsRefreshTimer);
      wsRefreshTimer = null;
    }
    stopDomainInvalidation?.();
    stopDomainInvalidation = null;
    disconnectDevicesUpdates();
  };

  const requestUnregister = (camera: CameraRow): void => {
    if (options.readPendingUnregister() || actionIsBusy(camera.id, 'unregister')) return;
    options.setPendingUnregister(camera);
  };

  const closeUnregisterDialog = (): void => {
    options.setPendingUnregister(null);
  };

  const sessionRefFromCamera = (camera: CameraRow): string | null => {
    const id = camera.captureSessionId?.toString().trim() ?? '';
    if (id.startsWith('peer:')) return null;
    const alias = camera.captureSessionAlias?.trim();
    if (alias) return alias;
    return id.length ? id : null;
  };

  const unregisterCameraStream = async (camera: CameraRow): Promise<void> => {
    const streamId = camera.captureSessionId?.toString().trim() ?? '';
    if (!streamId) {
      toaster.error({
        title: 'Stream not active',
        description: 'This camera does not have an active capture session.'
      });
      return;
    }
    if (actionIsBusy(camera.id, 'unregister')) return;
    setActionBusy(camera.id, 'unregister', true);
    try {
      await StreamsApi.deleteStream({ id: streamId });
      toaster.success({
        title: 'Stream deleted',
        description: `${camera.name} capture session removed`
      });
      invalidateOwnedStreamMutationResources();
      await refreshDevices();
    } catch (error) {
      reportError({
        title: 'Delete failed',
        error,
        fallback: 'Unable to delete the stream right now.'
      });
    } finally {
      setActionBusy(camera.id, 'unregister', false);
    }
  };

  const confirmUnregister = async (): Promise<void> => {
    const target = options.readPendingUnregister();
    if (!target) return;
    await unregisterCameraStream(target);
    options.setPendingUnregister(null);
  };

  const downloadCameraManifest = async (camera: CameraRow): Promise<void> => {
    const streamId = camera.captureSessionId?.toString().trim() ?? '';
    if (!streamId) {
      toaster.error({
        title: 'Manifest unavailable',
        description: 'No capture session is active for this camera.'
      });
      return;
    }
    if (actionIsBusy(camera.id, 'download')) return;
    setActionBusy(camera.id, 'download', true);
    try {
      const response = await StreamsApi.getStream({ id: streamId });
      const manifest = response?.manifest;
      if (!manifest || typeof manifest !== 'object') {
        throw new Error('Manifest not available');
      }
      const pretty = JSON.stringify(manifest, null, 2);
      const blob = new Blob([pretty], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const suggestedName =
        typeof manifest === 'object' &&
        manifest &&
        'path' in manifest &&
        typeof manifest.path === 'string'
          ? manifest.path.split('/').pop() ?? 'stream-manifest'
          : 'stream-manifest';
      const filename = `${suggestedName.replace(/\.json$/i, '') || 'stream-manifest'}-${camera.id}.json`;
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = filename;
      document.body.appendChild(anchor);
      anchor.click();
      document.body.removeChild(anchor);
      URL.revokeObjectURL(url);
      toaster.success({
        title: 'Manifest downloaded',
        description: `${camera.name} manifest saved`
      });
    } catch (error) {
      reportError({
        title: 'Download failed',
        error,
        fallback: 'Unable to download the manifest right now.'
      });
    } finally {
      setActionBusy(camera.id, 'download', false);
    }
  };

  const restoreCameraStreamResources = async (camera: CameraRow): Promise<void> => {
    const streamId = camera.captureSessionId?.toString().trim() ?? '';
    if (!streamId) {
      toaster.error({
        title: 'Restore unavailable',
        description: 'This camera does not have an active stream ID.'
      });
      return;
    }
    if (actionIsBusy(camera.id, 'restore')) return;
    setActionBusy(camera.id, 'restore', true);
    try {
      await apiFetch(`/device/resource-guard/restore/${encodeURIComponent(streamId)}`, {
        method: 'POST'
      });
      toaster.success({
        title: 'Stream resources re-enabled',
        description: `${camera.name} codecs restored`
      });
      await refreshResourceGuardStatus();
      await refreshDevices({ force: true });
    } catch (error) {
      reportError({
        title: 'Restore failed',
        error,
        fallback: 'Unable to re-enable stream resources right now.'
      });
    } finally {
      setActionBusy(camera.id, 'restore', false);
    }
  };

  return {
    start,
    stop,
    refreshDevices,
    refreshLocalizationConfig,
    refreshResourceGuardStatus,
    actionIsBusy,
    requestUnregister,
    closeUnregisterDialog,
    confirmUnregister,
    sessionRefFromCamera,
    downloadCameraManifest,
    restoreCameraStreamResources
  };
};
