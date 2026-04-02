import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { get } from 'svelte/store';

import {
  dismissedOsHealthFingerprint,
  dismissOsHealthBanner,
  hydrateAppShellPreferences,
  osHealthFingerprint,
  resetAppShellStateForTesting,
  setBootloaderStatus,
  setOsHealthStatus,
  setSidebarCollapsed,
  showSettingsBootloaderWarning,
  showSettingsOsWarning,
  sidebarCollapsed,
  toggleSidebar
} from './appShellState';

type WindowValue = typeof globalThis extends { window: infer T } ? T : never;

const originalWindowDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'window');

const createStorage = (): Storage => {
  const data = new Map<string, string>();
  return {
    get length() {
      return data.size;
    },
    clear() {
      data.clear();
    },
    getItem(key: string) {
      return data.has(key) ? data.get(key)! : null;
    },
    key(index: number) {
      return Array.from(data.keys())[index] ?? null;
    },
    removeItem(key: string) {
      data.delete(key);
    },
    setItem(key: string, value: string) {
      data.set(key, value);
    }
  } as Storage;
};

const installWindow = (value: WindowValue): void => {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value
  });
};

describe('app shell state', () => {
  beforeEach(() => {
    resetAppShellStateForTesting();
    installWindow({ localStorage: createStorage() } as WindowValue);
  });

  afterEach(() => {
    resetAppShellStateForTesting();
    if (originalWindowDescriptor) {
      Object.defineProperty(globalThis, 'window', originalWindowDescriptor);
      return;
    }
    Reflect.deleteProperty(globalThis, 'window');
  });

  test('hydrates shell preferences from storage', () => {
    window.localStorage.setItem('helios.app.sidebar.collapsed', '1');
    window.localStorage.setItem('helios.app.os-health.dismissed', 'issue-a');

    hydrateAppShellPreferences();

    expect(get(sidebarCollapsed)).toBe(true);
    expect(get(dismissedOsHealthFingerprint)).toBe('issue-a');
  });

  test('persists sidebar toggles and banner dismissals', () => {
    setOsHealthStatus({
      status: 'degraded',
      issues: [{ code: 'rootfs_ro', description: 'Root filesystem is read only.' }]
    });

    toggleSidebar();
    dismissOsHealthBanner();

    expect(get(sidebarCollapsed)).toBe(true);
    expect(window.localStorage.getItem('helios.app.sidebar.collapsed')).toBe('1');
    expect(get(dismissedOsHealthFingerprint)).toBe(get(osHealthFingerprint));
    expect(window.localStorage.getItem('helios.app.os-health.dismissed')).toBe(
      'rootfs_ro:Root filesystem is read only.'
    );
  });

  test('derives settings warnings from bootloader and os health state', () => {
    setBootloaderStatus({
      supported: true,
      needs_update: true,
      staged: false,
      update_available: true
    });
    setOsHealthStatus({
      status: 'degraded',
      issues: [{ code: 'boot_partition_low', description: 'Boot partition is nearly full.' }]
    });

    expect(get(showSettingsBootloaderWarning)).toBe(true);
    expect(get(showSettingsOsWarning)).toBe(true);

    setSidebarCollapsed(true);
    expect(window.localStorage.getItem('helios.app.sidebar.collapsed')).toBe('1');
  });
});
