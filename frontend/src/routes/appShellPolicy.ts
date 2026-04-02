import type { BootloaderStatus } from '$lib/ts-bindings/http/client';
import type { OsHealthStatus, ResourceGuardStatus } from '$lib/api/deviceStatusResources';

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

type WindowErrorLike = Event & {
  error?: unknown;
  message?: unknown;
};

type PromiseRejectionLike = Event & {
  reason?: unknown;
};

type RuntimeErrorNotifier = {
  handleWindowError: (event: Event) => void;
  handleUnhandledRejection: (event: Event) => void;
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

const asWindowError = (event: Event): WindowErrorLike | null => {
  if (typeof event !== 'object' || event === null) return null;
  if (!('error' in event) && !('message' in event)) return null;
  return event as WindowErrorLike;
};

const asPromiseRejection = (event: Event): PromiseRejectionLike | null => {
  if (typeof event !== 'object' || event === null) return null;
  if (!('reason' in event)) return null;
  return event as PromiseRejectionLike;
};

export const needsBootloaderAttention = (status: BootloaderStatus | null): boolean =>
  Boolean(status?.supported && status?.needs_update);

export const createRuntimeErrorNotifier = (
  notifyRuntimeError: (message: string) => void,
  now: () => number = () => Date.now()
): RuntimeErrorNotifier => {
  const recentRuntimeErrors = new Map<string, number>();

  const shouldToastRuntimeError = (key: string): boolean => {
    const currentTime = now();
    const previous = recentRuntimeErrors.get(key) ?? 0;
    recentRuntimeErrors.set(key, currentTime);
    for (const [messageKey, timestamp] of recentRuntimeErrors.entries()) {
      if (currentTime - timestamp > RUNTIME_ERROR_TOAST_THROTTLE_MS * 4) {
        recentRuntimeErrors.delete(messageKey);
      }
    }
    return currentTime - previous >= RUNTIME_ERROR_TOAST_THROTTLE_MS;
  };

  return {
    handleWindowError: (event: Event): void => {
      const errorEvent = asWindowError(event);
      if (!errorEvent) return;
      if (isIgnorableRuntimeError(errorEvent.error, typeof errorEvent.message === 'string' ? errorEvent.message : undefined)) {
        return;
      }
      const message = runtimeErrorMessage(
        errorEvent.error,
        typeof errorEvent.message === 'string' ? errorEvent.message : undefined
      );
      if (!shouldToastRuntimeError(message)) return;
      notifyRuntimeError(message);
    },
    handleUnhandledRejection: (event: Event): void => {
      const rejectionEvent = asPromiseRejection(event);
      const reason = rejectionEvent?.reason;
      if (isIgnorableRuntimeError(reason, 'Unhandled promise rejection')) return;
      const message = runtimeErrorMessage(reason, 'Unhandled promise rejection');
      if (!shouldToastRuntimeError(message)) return;
      notifyRuntimeError(message);
    }
  };
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
