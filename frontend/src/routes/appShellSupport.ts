import { updated } from '$app/stores';
import {
  bootloaderStatusResource,
  osHealthStatusResource,
  resourceGuardStatusResource,
  type OsHealthStatus,
  type ResourceGuardStatus
} from '$lib/api/deviceStatusResources';
import { startDomainInvalidationBridge } from '$lib/api/invalidation';
import { startRefreshScheduler } from '$lib/api/refreshScheduler';
import { readStorage, writeStorage } from '$lib/utils/storage';
import type { BootloaderStatus } from '$lib/ts-bindings/http/client';
import { SvelteMap } from 'svelte/reactivity';

export const SIDEBAR_COLLAPSED_STORAGE_KEY = 'helios.app.sidebar.collapsed';
export const OS_HEALTH_BANNER_DISMISSED_STORAGE_KEY = 'helios.app.os-health.dismissed';
const RUNTIME_ERROR_TOAST_THROTTLE_MS = 5_000;

export type ResourceGuardBannerState = {
  title: string;
  details: string;
};

export type OsHealthBannerState = {
  title: string;
  details: string;
};

type RuntimeErrorNotifier = {
  handleWindowError: (event: Event) => void;
  handleUnhandledRejection: (event: PromiseRejectionEvent) => void;
};

type AppShellBootstrapOptions = {
  setIsSidebarCollapsed: (value: boolean) => void;
  setDismissedOsHealthFingerprint: (value: string) => void;
  setBootloaderStatus: (value: BootloaderStatus | null) => void;
  setOsHealthStatus: (value: OsHealthStatus | null) => void;
  setResourceGuardStatus: (value: ResourceGuardStatus | null) => void;
  notifyRuntimeError: (message: string) => void;
  reloadWindow?: () => void;
};

const normalizeRuntimeMessage = (value: string): string => {
  const trimmed = value.trim();
  if (!trimmed.length) return 'Unexpected UI error';
  return trimmed.length > 320 ? `${trimmed.slice(0, 320)}...` : trimmed;
};

const isIgnorableRuntimeMessage = (value: string): boolean => {
  const normalized = value.trim().toLowerCase();
  return (
    normalized.includes('resizeobserver loop completed with undelivered notifications') ||
    normalized.includes('resizeobserver loop limit exceeded') ||
    normalized === 'the operation was aborted.' ||
    normalized === 'operation was aborted' ||
    normalized === 'signal is aborted without reason'
  );
};

const runtimeErrorMessage = (error: unknown, fallback?: string): string => {
  if (error instanceof Error && typeof error.message === 'string' && error.message.trim().length) {
    return normalizeRuntimeMessage(error.message);
  }
  if (typeof error === 'string' && error.trim().length) {
    return normalizeRuntimeMessage(error);
  }
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string' && message.trim().length) {
      return normalizeRuntimeMessage(message);
    }
  }
  return normalizeRuntimeMessage(fallback ?? 'Unexpected UI error');
};

const isIgnorableRuntimeError = (error: unknown, fallback?: string): boolean => {
  const message = runtimeErrorMessage(error, fallback);
  if (isIgnorableRuntimeMessage(message)) {
    return true;
  }
  if (error && typeof error === 'object' && 'name' in error) {
    const name = String((error as { name?: unknown }).name ?? '').trim().toLowerCase();
    if (name === 'aborterror') {
      return true;
    }
  }
  return false;
};

const createRuntimeErrorNotifier = (notifyRuntimeError: (message: string) => void): RuntimeErrorNotifier => {
  const recentRuntimeErrors = new SvelteMap<string, number>();

  const shouldToastRuntimeError = (key: string): boolean => {
    const now = Date.now();
    const previous = recentRuntimeErrors.get(key) ?? 0;
    recentRuntimeErrors.set(key, now);
    for (const [messageKey, timestamp] of recentRuntimeErrors.entries()) {
      if (now - timestamp > RUNTIME_ERROR_TOAST_THROTTLE_MS * 4) {
        recentRuntimeErrors.delete(messageKey);
      }
    }
    return now - previous >= RUNTIME_ERROR_TOAST_THROTTLE_MS;
  };

  return {
    handleWindowError: (event: Event): void => {
      if (!(event instanceof ErrorEvent)) return;
      if (isIgnorableRuntimeError(event.error, event.message)) return;
      const message = runtimeErrorMessage(event.error, event.message);
      if (!shouldToastRuntimeError(message)) return;
      notifyRuntimeError(message);
    },
    handleUnhandledRejection: (event: PromiseRejectionEvent): void => {
      if (isIgnorableRuntimeError(event.reason, 'Unhandled promise rejection')) return;
      const message = runtimeErrorMessage(event.reason, 'Unhandled promise rejection');
      if (!shouldToastRuntimeError(message)) return;
      notifyRuntimeError(message);
    }
  };
};

const hydrateCachedStatuses = (options: AppShellBootstrapOptions): void => {
  const cachedBootloader = bootloaderStatusResource.read();
  if (cachedBootloader?.data) {
    options.setBootloaderStatus(cachedBootloader.data);
  }
  const cachedOsHealth = osHealthStatusResource.read();
  if (cachedOsHealth?.data) {
    options.setOsHealthStatus(cachedOsHealth.data);
  }
  const cachedResourceGuard = resourceGuardStatusResource.read();
  if (cachedResourceGuard?.data) {
    options.setResourceGuardStatus(cachedResourceGuard.data);
  }
};

const refreshBootloaderStatus = async (
  setBootloaderStatus: (value: BootloaderStatus | null) => void
): Promise<void> => {
  try {
    setBootloaderStatus(await bootloaderStatusResource.refresh());
  } catch {
    setBootloaderStatus(null);
  }
};

const refreshOsHealthStatus = async (
  setOsHealthStatus: (value: OsHealthStatus | null) => void
): Promise<void> => {
  try {
    setOsHealthStatus(await osHealthStatusResource.refresh());
  } catch {
    setOsHealthStatus(null);
  }
};

const refreshResourceGuardStatus = async (
  setResourceGuardStatus: (value: ResourceGuardStatus | null) => void
): Promise<void> => {
  try {
    setResourceGuardStatus(await resourceGuardStatusResource.refresh());
  } catch {
    setResourceGuardStatus(null);
  }
};

export const bootstrapAppShell = (options: AppShellBootstrapOptions): (() => void) => {
  startDomainInvalidationBridge();
  options.setIsSidebarCollapsed(readStorage(SIDEBAR_COLLAPSED_STORAGE_KEY) === '1');
  options.setDismissedOsHealthFingerprint(readStorage(OS_HEALTH_BANNER_DISMISSED_STORAGE_KEY));
  hydrateCachedStatuses(options);

  const stopBootloaderRefresh = startRefreshScheduler(
    () => refreshBootloaderStatus(options.setBootloaderStatus),
    {
      intervalMs: 120_000,
      immediate: true
    }
  );
  const stopOsHealthRefresh = startRefreshScheduler(() => refreshOsHealthStatus(options.setOsHealthStatus), {
    intervalMs: 10_000,
    immediate: true
  });
  const stopResourceGuardRefresh = startRefreshScheduler(
    () => refreshResourceGuardStatus(options.setResourceGuardStatus),
    {
      intervalMs: 4_000,
      immediate: true
    }
  );
  const stopVersionWatch = updated.subscribe((isUpdated) => {
    if (!isUpdated) return;
    if (typeof options.reloadWindow === 'function') {
      options.reloadWindow();
      return;
    }
    globalThis.location?.reload();
  });

  const runtimeErrorNotifier = createRuntimeErrorNotifier(options.notifyRuntimeError);
  window.addEventListener('error', runtimeErrorNotifier.handleWindowError);
  window.addEventListener('unhandledrejection', runtimeErrorNotifier.handleUnhandledRejection);

  return () => {
    stopBootloaderRefresh();
    stopOsHealthRefresh();
    stopResourceGuardRefresh();
    stopVersionWatch();
    window.removeEventListener('error', runtimeErrorNotifier.handleWindowError);
    window.removeEventListener('unhandledrejection', runtimeErrorNotifier.handleUnhandledRejection);
  };
};

export const persistSidebarCollapsed = (collapsed: boolean): void => {
  writeStorage(SIDEBAR_COLLAPSED_STORAGE_KEY, collapsed ? '1' : '0');
};

export const persistDismissedOsHealthFingerprint = (fingerprint: string): void => {
  writeStorage(OS_HEALTH_BANNER_DISMISSED_STORAGE_KEY, fingerprint);
};

export function buildOsHealthBanner(status: OsHealthStatus | null): OsHealthBannerState | null {
  const issues = Array.isArray(status?.issues) ? status.issues : [];
  if (!issues.length) return null;

  const primary = issues[0];
  const extraCount = Math.max(0, issues.length - 1);
  const title = primary?.code?.trim().length ? `Core OS issue: ${primary.code}` : 'Core OS issue detected';
  const details = primary?.description?.trim().length
    ? extraCount > 0
      ? `${primary.description} ${extraCount} additional issue${extraCount === 1 ? '' : 's'} reported.`
      : primary.description
    : extraCount > 0
      ? `${extraCount + 1} core OS issues reported.`
      : 'Device storage or boot state is degraded.';

  return { title, details };
}

export function buildOsHealthFingerprint(status: OsHealthStatus | null): string {
  const issues = Array.isArray(status?.issues) ? status.issues : [];
  return issues
    .map((issue) => `${issue.code?.trim() ?? ''}:${issue.description?.trim() ?? ''}`)
    .filter((entry) => entry.length > 1)
    .sort()
    .join('|');
}

export function buildResourceGuardBanner(status: ResourceGuardStatus | null): ResourceGuardBannerState | null {
  if (!status?.enabled) return null;
  const degraded = Array.isArray(status.degraded_streams) ? status.degraded_streams : [];
  const action = status.last_action ?? null;
  const recentlyIntervened = Boolean(action && Date.now() - action.at_ms <= 180_000);
  if (!degraded.length && !recentlyIntervened) return null;

  const title =
    degraded.length > 0
      ? `Resource guard active: ${degraded.length} stream${degraded.length === 1 ? '' : 's'} degraded`
      : 'Resource guard intervened to protect device stability';

  if (!action) {
    return { title, details: 'Resource pressure mitigation is active.' };
  }

  const actionLabel =
    action.kind === 'disable_decoder'
      ? 'Disabled decoder'
      : action.kind === 'disable_all_codecs'
        ? 'Disabled codecs'
        : action.kind === 'stop_stream'
          ? 'Stopped stream'
          : 'Restored codecs';
  const streamLabel = action.alias?.trim()?.length ? action.alias.trim() : action.stream_id;
  const memoryLabel =
    typeof action.mem_available_kb === 'number'
      ? `MemAvailable ${action.mem_available_kb.toLocaleString()} kB.`
      : typeof status.last_mem_available_kb === 'number'
        ? `MemAvailable ${status.last_mem_available_kb.toLocaleString()} kB.`
        : '';
  const details = `${actionLabel} on ${streamLabel}. ${action.reason}${memoryLabel ? ` ${memoryLabel}` : ''}`;
  return { title, details };
}
