import { describe, expect, test } from 'bun:test';

import type { SystemsRuntimeSnapshot } from '$lib/types/systems';

import { buildActivityTabs } from './systemsPageTabs';

function runtimeWithCapabilities(
  overrides: Partial<SystemsRuntimeSnapshot['capabilities']> = {}
): SystemsRuntimeSnapshot {
  return {
    platform: {
      family: 'unknown',
      model: null,
      architecture: 'unknown'
    },
    capabilities: {
      logs: true,
      console: true,
      processes: true,
      sensors: false,
      i2c: false,
      imu: false,
      updater: false,
      resourceGuard: true,
      activeRoot: false,
      ...overrides
    },
    policies: {
      logFilter: 'info',
      apiTokio: { workerThreads: 0, maxBlockingThreads: 0, threadStackBytes: null, blockingKeepAliveMs: null },
      engineTokio: { workerThreads: 0, maxBlockingThreads: 0, threadStackBytes: null, blockingKeepAliveMs: null },
      peripheralsTokio: { workerThreads: 0, maxBlockingThreads: 0, threadStackBytes: null, blockingKeepAliveMs: null },
      startupCacheWarm: { initialDelayMs: 0, retryDelayMs: 0, attempts: 0 },
      logSources: { cacheMs: 0, refreshTimeoutMs: 0 },
      i2cInventory: { timeoutMs: 0, cacheTtlMs: 0 },
      imu: { idleIntervalMs: 0 },
      resourceGuard: {
        enabled: false,
        pollMs: 0,
        memLowKb: 0,
        memRecoverKb: 0,
        cooldownMs: 0,
        metricsTopN: 0,
        metricsTimeoutMs: 0,
        allowStopFallback: false,
        stopTimeoutMs: 0
      },
      styxCapture: {
        queueDepth: null,
        poolMin: null,
        poolBytes: null,
        poolSpare: null,
        anyOverridden: false
      }
    },
    observability: {
      health: {
        ok: false,
        serverTimeMs: 0,
        uptimeMs: 0,
        version: '',
        shadowRecorder: false,
        pipelineRegistryStartupWarm: false,
        pipelineRegistryPrefetch: false,
        apiToolsHelperOk: false,
        apiToolsHelperPath: ''
      },
      streams: {
        streamCount: 0,
        codecCount: 0,
        stale: true,
        revision: 0
      },
      os: {
        versionId: null,
        buildId: null,
        prettyName: null,
        activeRoot: null
      },
      resourceGuard: {
        enabled: false,
        pressureActive: false,
        degradedStreamCount: 0,
        recentActionCount: 0,
        lastMemAvailableKb: null
      },
      logSourceCount: 0,
      logSourcesFreshness: {
        state: 'unavailable',
        reason: 'refresh_error',
        observedAtMs: 0,
        lastSuccessAtMs: null
      },
      logSourcesRevision: 0
    }
  };
}

describe('systems page activity tabs', () => {
  test('always includes runtime and log visibility tabs', () => {
    const tabs = buildActivityTabs(runtimeWithCapabilities());

    expect(tabs.map((tab) => tab.id)).toEqual(['runtime', 'logs', 'console', 'processes']);
  });

  test('adds hardware tabs from runtime capability discovery', () => {
    const runtime = runtimeWithCapabilities({ i2c: true, imu: true });

    const tabs = buildActivityTabs(runtime);

    expect(tabs.map((tab) => tab.id)).toEqual(['runtime', 'logs', 'i2c', 'imu', 'console', 'processes']);
  });

  test('removes console and process tabs when runtime disables them', () => {
    const runtime = runtimeWithCapabilities({ console: false, processes: false });

    const tabs = buildActivityTabs(runtime);

    expect(tabs.map((tab) => tab.id)).toEqual(['runtime', 'logs']);
  });
});
