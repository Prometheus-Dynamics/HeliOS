import type { BootloaderStatus } from '$lib/api/client';
import type { OsHealthStatus, ResourceGuardStatus } from '$lib/api/deviceStatusResources';
import { readStorage, writeStorage } from '$lib/utils/storage';
import { derived, get, readonly, writable } from 'svelte/store';

import {
  buildOsHealthBanner,
  buildOsHealthFingerprint,
  buildResourceGuardBanner,
  needsBootloaderAttention,
  OS_HEALTH_BANNER_DISMISSED_STORAGE_KEY,
  SIDEBAR_COLLAPSED_STORAGE_KEY
} from './appShellPolicy';

const sidebarCollapsedState = writable(false);
const dismissedOsHealthFingerprintState = writable('');
const bootloaderStatusState = writable<BootloaderStatus | null>(null);
const osHealthStatusState = writable<OsHealthStatus | null>(null);
const resourceGuardStatusState = writable<ResourceGuardStatus | null>(null);

export const sidebarCollapsed = readonly(sidebarCollapsedState);
export const dismissedOsHealthFingerprint = readonly(dismissedOsHealthFingerprintState);
export const bootloaderStatus = readonly(bootloaderStatusState);
export const osHealthStatus = readonly(osHealthStatusState);
export const resourceGuardStatus = readonly(resourceGuardStatusState);

export const osHealthBanner = derived(osHealthStatusState, (status) => buildOsHealthBanner(status));
export const osHealthFingerprint = derived(osHealthStatusState, (status) => buildOsHealthFingerprint(status));
export const resourceGuardBanner = derived(resourceGuardStatusState, (status) => buildResourceGuardBanner(status));
export const showSettingsBootloaderWarning = derived(bootloaderStatusState, (status) => needsBootloaderAttention(status));
export const showSettingsOsWarning = derived(osHealthBanner, (banner) => Boolean(banner));

export const setSidebarCollapsed = (value: boolean): void => {
  sidebarCollapsedState.set(value);
  writeStorage(SIDEBAR_COLLAPSED_STORAGE_KEY, value ? '1' : '0');
};

export const setDismissedOsHealthFingerprint = (value: string): void => {
  dismissedOsHealthFingerprintState.set(value);
  writeStorage(OS_HEALTH_BANNER_DISMISSED_STORAGE_KEY, value);
};

export const setBootloaderStatus = (value: BootloaderStatus | null): void => {
  bootloaderStatusState.set(value);
};

export const setOsHealthStatus = (value: OsHealthStatus | null): void => {
  osHealthStatusState.set(value);
};

export const setResourceGuardStatus = (value: ResourceGuardStatus | null): void => {
  resourceGuardStatusState.set(value);
};

export const hydrateAppShellPreferences = (): void => {
  sidebarCollapsedState.set(readStorage(SIDEBAR_COLLAPSED_STORAGE_KEY) === '1');
  dismissedOsHealthFingerprintState.set(readStorage(OS_HEALTH_BANNER_DISMISSED_STORAGE_KEY) ?? '');
};

export const toggleSidebar = (): void => {
  setSidebarCollapsed(!get(sidebarCollapsedState));
};

export const dismissOsHealthBanner = (): void => {
  setDismissedOsHealthFingerprint(get(osHealthFingerprint));
};

export const resetAppShellStateForTesting = (): void => {
  sidebarCollapsedState.set(false);
  dismissedOsHealthFingerprintState.set('');
  bootloaderStatusState.set(null);
  osHealthStatusState.set(null);
  resourceGuardStatusState.set(null);
};
