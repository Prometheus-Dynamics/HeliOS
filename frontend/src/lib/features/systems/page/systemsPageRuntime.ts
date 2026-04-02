import { createDomainResource } from '$lib/api/domainResources';
import { fetchI2cInventorySnapshot, fetchSystemsRuntimeSnapshot, refreshI2cInventory, refreshImuStatus } from '$lib/api/systemsPage';
import { scheduleWhenIdle } from '$lib/utils/browserSchedule';
import type { ImuStatus, SystemsPageData } from '$lib/types/systems';
import { reportError } from '$lib/ui/errorPolicy';
import { formatLoadError } from './systemsPageUtils';

const I2C_CACHE_KEY = 'systems:i2c:v1';
const IMU_CACHE_KEY = 'systems:imu:v1';
const RUNTIME_CACHE_KEY = 'systems:runtime:v1';
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

const runtimeResource = createDomainResource({
  key: RUNTIME_CACHE_KEY,
  loader: fetchSystemsRuntimeSnapshot,
  staleMs: SYSTEMS_CACHE_STALE_MS,
  maxAgeMs: SYSTEMS_CACHE_MAX_MS,
  kinds: ['device', 'settings', 'streams']
});

type SystemsPageRuntimeOptions = {
  readSystems: () => SystemsPageData;
  setSystems: (value: SystemsPageData) => void;
  readHasLoadedOnce: () => boolean;
  setHasLoadedOnce: (value: boolean) => void;
  readIsRefreshing: () => boolean;
  setIsRefreshing: (value: boolean) => void;
  readIsRescanningI2c: () => boolean;
  setIsRescanningI2c: (value: boolean) => void;
  setLoadError: (value: string | null) => void;
  setRuntimeLoading: (value: boolean) => void;
  setI2cLoading: (value: boolean) => void;
  setImuLoading: (value: boolean) => void;
  setRuntimeError: (value: string | null) => void;
  setI2cError: (value: string | null) => void;
  readConnectionStatus: () => string;
  readIsBackendUnavailable: () => boolean;
  applyImuStatus: (status: ImuStatus) => void;
};

export const createSystemsPageRuntime = (options: SystemsPageRuntimeOptions) => {
  let stopDomainInvalidation: (() => void) | null = null;
  let cancelBootstrapRefresh: (() => void) | null = null;

  const refreshSystems = async (
    runtimeOptions: { bootstrap?: boolean } = {}
  ): Promise<void> => {
    const { bootstrap = false } = runtimeOptions;
    if (options.readIsRefreshing()) return;
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
    let failures = 0;
    let sawSuccess = false;
    let runtime = options.readSystems().runtime;

    options.setRuntimeLoading(true);
    try {
      runtime = await runtimeResource.refresh();
      sawSuccess = true;
      options.setLoadError(null);
      options.setRuntimeError(null);
      options.setSystems({
        ...options.readSystems(),
        runtime,
        errors: { ...(options.readSystems().errors ?? {}), runtime: null },
        fetchedAt: Date.now()
      });
    } catch (error) {
      failures += 1;
      if (options.readConnectionStatus() === 'online') {
        reportError({ context: 'Systems runtime refresh', error, toast: false });
      }
      const message = formatLoadError(error);
      options.setRuntimeError(message);
      options.setSystems({
        ...options.readSystems(),
        errors: { ...(options.readSystems().errors ?? {}), runtime: message }
      });
      runtimeResource.invalidate();
    } finally {
      options.setRuntimeLoading(false);
    }

    const shouldRefreshI2c = runtime.capabilities.i2c;
    const shouldRefreshImu = runtime.capabilities.imu;
    const followupTasks: Array<Promise<void>> = [];

    if (shouldRefreshI2c) {
      options.setI2cLoading(true);
      followupTasks.push(
        i2cResource
          .refresh()
          .then((inventory) => {
            sawSuccess = true;
            options.setLoadError(null);
            options.setI2cError(null);
            options.setSystems({
              ...options.readSystems(),
              i2cInventory: inventory,
              errors: { ...(options.readSystems().errors ?? {}), i2c: null },
              fetchedAt: Date.now()
            });
          })
          .catch((error) => {
            failures += 1;
            if (options.readConnectionStatus() === 'online') {
              reportError({ context: 'Systems I2C refresh', error, toast: false });
            }
            const message = formatLoadError(error);
            const visibleInventory =
              options.readSystems().i2cInventory.buses.length > 0 ||
              options.readSystems().i2cInventory.devices.length > 0;
            options.setI2cError(visibleInventory ? null : message);
            options.setSystems({
              ...options.readSystems(),
              errors: { ...(options.readSystems().errors ?? {}), i2c: visibleInventory ? null : message }
            });
            i2cResource.invalidate();
          })
          .finally(() => {
            options.setI2cLoading(false);
          })
      );
    }

    if (shouldRefreshImu) {
      options.setImuLoading(true);
      followupTasks.push(
        imuResource
          .refresh()
          .then((status) => {
            sawSuccess = true;
            options.setLoadError(null);
            options.applyImuStatus(status);
          })
          .catch((error) => {
            failures += 1;
            if (options.readConnectionStatus() === 'online') {
              reportError({ context: 'Systems IMU refresh', error, toast: false });
            }
            const message = formatLoadError(error);
            options.setSystems({
              ...options.readSystems(),
              errors: { ...(options.readSystems().errors ?? {}), imu: message }
            });
            imuResource.invalidate();
          })
          .finally(() => {
            options.setImuLoading(false);
          })
      );
    }

    await Promise.allSettled(followupTasks);
    const refreshCount = 1 + Number(shouldRefreshI2c) + Number(shouldRefreshImu);
    options.setIsRefreshing(false);
    if (bootstrap || !options.readHasLoadedOnce()) {
      options.setHasLoadedOnce(true);
    }
    if (!sawSuccess && failures >= refreshCount) {
      options.setLoadError('Systems data unavailable');
    }
  };

  const rescanI2c = async (): Promise<void> => {
    if (!options.readSystems().runtime.capabilities.i2c) return;
    if (options.readIsRescanningI2c()) return;
    const systems = options.readSystems();
    const visibleInventory =
      systems.i2cInventory.buses.length > 0 || systems.i2cInventory.devices.length > 0;
    options.setI2cError(null);
    options.setIsRescanningI2c(true);
    try {
      const inventory = await refreshI2cInventory();
      options.setSystems({
        ...options.readSystems(),
        i2cInventory: inventory,
        errors: { ...(options.readSystems().errors ?? {}), i2c: null },
        fetchedAt: Date.now()
      });
    } catch (error) {
      reportError({ context: 'I2C rescan', error, toast: false });
      const message = formatLoadError(error);
      options.setI2cError(visibleInventory ? null : message);
      options.setSystems({
        ...options.readSystems(),
        errors: { ...(options.readSystems().errors ?? {}), i2c: visibleInventory ? null : message }
      });
    } finally {
      options.setIsRescanningI2c(false);
    }
  };

  const start = (): (() => void) => {
    const cachedRuntime = runtimeResource.read();
    if (cachedRuntime?.data) {
      options.setSystems({
        ...options.readSystems(),
        runtime: cachedRuntime.data,
        fetchedAt: cachedRuntime.fetchedAt
      });
      options.setRuntimeError(null);
      options.setHasLoadedOnce(true);
    }

    const cachedI2c = i2cResource.read();
    if (cachedI2c?.data) {
      options.setSystems({
        ...options.readSystems(),
        i2cInventory: cachedI2c.data,
        fetchedAt: cachedI2c.fetchedAt
      });
      options.setI2cError(null);
      options.setHasLoadedOnce(true);
    }

    const cachedImu = imuResource.read();
    if (cachedImu?.data) {
      options.setSystems({
        ...options.readSystems(),
        imu: cachedImu.data,
        fetchedAt: cachedImu.fetchedAt
      });
      options.applyImuStatus(cachedImu.data);
      options.setHasLoadedOnce(true);
    }

    if (options.readHasLoadedOnce()) {
      cancelBootstrapRefresh = scheduleWhenIdle(() => {
        if (!document.hidden) {
          void refreshSystems();
        }
      }, { timeoutMs: 1800, fallbackMs: 650 });
    } else {
      void refreshSystems({ bootstrap: true });
    }

    const stopRuntimeInvalidations = runtimeResource.subscribeInvalidations(() => {
      void refreshSystems();
    }, { debounceMs: 250 });
    const stopI2cInvalidations = i2cResource.subscribeInvalidations(() => {
      void refreshSystems();
    }, { debounceMs: 250 });
    const stopImuInvalidations = imuResource.subscribeInvalidations(() => {
      void refreshSystems();
    }, { debounceMs: 250 });
    stopDomainInvalidation = () => {
      stopRuntimeInvalidations();
      stopI2cInvalidations();
      stopImuInvalidations();
    };

    return stop;
  };

  const stop = (): void => {
    cancelBootstrapRefresh?.();
    cancelBootstrapRefresh = null;
    stopDomainInvalidation?.();
    stopDomainInvalidation = null;
  };

  return {
    start,
    stop,
    refreshSystems,
    rescanI2c
  };
};
