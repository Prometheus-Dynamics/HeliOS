import { describe, expect, test } from 'bun:test';
import type { BootloaderStatus } from '$lib/ts-bindings/http/client';
import type { ResourceGuardStatus } from '$lib/api/deviceStatusResources';

import {
  buildOsHealthBanner,
  buildOsHealthFingerprint,
  buildResourceGuardBanner,
  createRuntimeErrorNotifier,
  needsBootloaderAttention
} from './appShellPolicy';

describe('app shell policy', () => {
  test('builds os health banner and fingerprint from issue list', () => {
    const status = {
      status: 'degraded',
      issues: [
        { code: 'rootfs_ro', description: 'Root filesystem is read only.' },
        { code: 'boot_partition_low', description: 'Boot partition is nearly full.' }
      ]
    };

    expect(buildOsHealthBanner(status)).toEqual({
      title: 'Core OS issue: rootfs_ro',
      details: 'Root filesystem is read only. 1 additional issue reported.'
    });
    expect(buildOsHealthFingerprint(status)).toBe(
      'boot_partition_low:Boot partition is nearly full.|rootfs_ro:Root filesystem is read only.'
    );
  });

  test('builds resource guard banner and bootloader attention states', () => {
    const bootloaderUpdateRequired: BootloaderStatus = {
      supported: true,
      needs_update: true,
      staged: false,
      update_available: true
    };
    const bootloaderHealthy: BootloaderStatus = {
      supported: true,
      needs_update: false,
      staged: false,
      update_available: false
    };
    const resourceGuardStatus: ResourceGuardStatus = {
      cooldown_ms: 60_000,
      enabled: true,
      degraded_streams: [
        {
          stream_id: 'cam-1',
          alias: 'Front Camera',
          changed_at_ms: 150_000,
          stage: 'decoder_disabled'
        }
      ],
      last_action: {
        at_ms: 150_000,
        kind: 'disable_decoder',
        reason: 'Memory pressure',
        score: 0.91,
        stream_id: 'cam-1',
        alias: 'Front Camera',
        mem_available_kb: 123_456
      },
      mem_low_kb: 65_536,
      mem_recover_kb: 131_072,
      last_mem_available_kb: 123_456,
      poll_ms: 4_000,
      pressure_active: true,
      recent_actions: []
    };
    const originalDateNow = Date.now;
    Date.now = () => 200_000;
    try {
      expect(needsBootloaderAttention(bootloaderUpdateRequired)).toBe(true);
      expect(needsBootloaderAttention(bootloaderHealthy)).toBe(false);

      expect(buildResourceGuardBanner(resourceGuardStatus)).toEqual({
        title: 'Resource guard active: 1 stream degraded',
        details: 'Disabled decoder on Front Camera. Memory pressure MemAvailable 123,456 kB.'
      });
    } finally {
      Date.now = originalDateNow;
    }
  });

  test('ignores noisy runtime errors and throttles duplicate toasts', () => {
    const messages: string[] = [];
    let currentTime = 10_000;
    const notifier = createRuntimeErrorNotifier(
      (message) => {
        messages.push(message);
      },
      () => currentTime
    );

    notifier.handleWindowError({
      error: new Error('ResizeObserver loop limit exceeded'),
      message: 'ResizeObserver loop limit exceeded'
    } as unknown as Event);

    notifier.handleWindowError({
      error: new Error('Exploded'),
      message: 'Exploded'
    } as unknown as Event);
    notifier.handleUnhandledRejection({
      reason: new Error('Exploded')
    } as unknown as Event);

    currentTime += 6_000;
    notifier.handleUnhandledRejection({
      reason: new Error('Exploded')
    } as unknown as Event);

    expect(messages).toEqual(['Exploded', 'Exploded']);
  });
});
