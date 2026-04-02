import { updated } from '$app/stores';
import {
  bootloaderStatusResource,
  osHealthStatusResource,
  resourceGuardStatusResource
} from '$lib/api/deviceStatusResources';
import { startDomainInvalidationBridge } from '$lib/api/invalidation';
import { startRefreshScheduler } from '$lib/api/refreshScheduler';

import { createRuntimeErrorNotifier } from './appShellPolicy';
import {
  hydrateAppShellPreferences,
  setBootloaderStatus,
  setOsHealthStatus,
  setResourceGuardStatus
} from './appShellState';

type AppShellRuntimeOptions = {
  notifyRuntimeError: (message: string) => void;
  reloadWindow?: () => void;
};

const hydrateCachedStatuses = (): void => {
  const cachedBootloader = bootloaderStatusResource.read();
  if (cachedBootloader?.data) {
    setBootloaderStatus(cachedBootloader.data);
  }

  const cachedOsHealth = osHealthStatusResource.read();
  if (cachedOsHealth?.data) {
    setOsHealthStatus(cachedOsHealth.data);
  }

  const cachedResourceGuard = resourceGuardStatusResource.read();
  if (cachedResourceGuard?.data) {
    setResourceGuardStatus(cachedResourceGuard.data);
  }
};

const refreshBootloaderStatus = async (): Promise<void> => {
  try {
    setBootloaderStatus(await bootloaderStatusResource.refresh());
  } catch {
    setBootloaderStatus(null);
  }
};

const refreshOsHealthStatus = async (): Promise<void> => {
  try {
    setOsHealthStatus(await osHealthStatusResource.refresh());
  } catch {
    setOsHealthStatus(null);
  }
};

const refreshResourceGuardStatus = async (): Promise<void> => {
  try {
    setResourceGuardStatus(await resourceGuardStatusResource.refresh());
  } catch {
    setResourceGuardStatus(null);
  }
};

export const startAppShellRuntime = (options: AppShellRuntimeOptions): (() => void) => {
  startDomainInvalidationBridge();
  hydrateAppShellPreferences();
  hydrateCachedStatuses();

  const stopBootloaderRefresh = startRefreshScheduler(refreshBootloaderStatus, {
    intervalMs: 120_000,
    immediate: true
  });
  const stopOsHealthRefresh = startRefreshScheduler(refreshOsHealthStatus, {
    intervalMs: 10_000,
    immediate: true
  });
  const stopResourceGuardRefresh = startRefreshScheduler(refreshResourceGuardStatus, {
    intervalMs: 4_000,
    immediate: true
  });
  const stopVersionWatch = updated.subscribe((isUpdated) => {
    if (!isUpdated) return;
    if (typeof options.reloadWindow === 'function') {
      options.reloadWindow();
      return;
    }
    globalThis.location?.reload();
  });

  const runtimeErrorNotifier = createRuntimeErrorNotifier(options.notifyRuntimeError);
  window.addEventListener('error', runtimeErrorNotifier.handleWindowError as EventListener);
  window.addEventListener('unhandledrejection', runtimeErrorNotifier.handleUnhandledRejection as EventListener);

  return () => {
    stopBootloaderRefresh();
    stopOsHealthRefresh();
    stopResourceGuardRefresh();
    stopVersionWatch();
    window.removeEventListener('error', runtimeErrorNotifier.handleWindowError as EventListener);
    window.removeEventListener('unhandledrejection', runtimeErrorNotifier.handleUnhandledRejection as EventListener);
  };
};
