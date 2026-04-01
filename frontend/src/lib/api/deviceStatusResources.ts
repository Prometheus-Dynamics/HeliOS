import { apiFetch } from '$lib/api/core/http';
import { createDomainResource } from '$lib/api/domainResources';
import { DeviceApi } from '$lib/api/deviceApi';
import { readModelFreshnessLabel } from '$lib/api/readModelFreshness';
import type {
  BootloaderStatus,
  DeviceHealthIssue,
  DeviceMetricsResponse,
  ResourceGuardAction,
  ResourceGuardActionKind,
  ResourceGuardDegradedStream,
  ResourceGuardStatus
} from '$lib/ts-bindings/http/client';

export type { ResourceGuardAction, ResourceGuardActionKind, ResourceGuardDegradedStream, ResourceGuardStatus };

export type OsHealthStatus = {
  status: string;
  issues: DeviceHealthIssue[];
};

const BOOTLOADER_STATUS_CACHE_KEY = 'device:bootloader:v1';
const RESOURCE_GUARD_STATUS_CACHE_KEY = 'device:resource-guard:v1';
const OS_HEALTH_STATUS_CACHE_KEY = 'device:os-health:v1';

export async function fetchBootloaderStatus(): Promise<BootloaderStatus> {
  return apiFetch<BootloaderStatus>('/device/bootloader');
}

export async function fetchResourceGuardStatus(): Promise<ResourceGuardStatus> {
  return apiFetch<ResourceGuardStatus>('/device/resource-guard');
}

export async function fetchOsHealthStatus(): Promise<OsHealthStatus> {
  const response = (await DeviceApi.metrics({ cacheMs: 0 })) as DeviceMetricsResponse;
  const metrics = response.metrics ?? null;
  const issues = Array.isArray(metrics?.issues)
    ? metrics.issues
        .map((issue) => ({
          code: typeof issue?.code === 'string' ? issue.code.trim() : '',
          description: typeof issue?.description === 'string' ? issue.description.trim() : ''
        }))
        .filter((issue) => issue.code.length > 0 || issue.description.length > 0)
    : [];

  const freshnessState = response.freshness?.state ?? null;
  const status =
    freshnessState && freshnessState !== 'live'
      ? readModelFreshnessLabel(response.freshness).toLowerCase()
      : typeof metrics?.status === 'string' && metrics.status.trim().length > 0
        ? metrics.status.trim()
        : issues.length > 0
          ? 'degraded'
          : 'healthy';
  return { status, issues };
}

export const bootloaderStatusResource = createDomainResource({
  key: BOOTLOADER_STATUS_CACHE_KEY,
  loader: fetchBootloaderStatus,
  staleMs: 60_000,
  maxAgeMs: 600_000,
  kinds: ['device', 'settings']
});

export const resourceGuardStatusResource = createDomainResource({
  key: RESOURCE_GUARD_STATUS_CACHE_KEY,
  loader: fetchResourceGuardStatus,
  staleMs: 4_000,
  maxAgeMs: 30_000,
  kinds: ['device', 'settings', 'streams']
});

export const osHealthStatusResource = createDomainResource({
  key: OS_HEALTH_STATUS_CACHE_KEY,
  loader: fetchOsHealthStatus,
  staleMs: 5_000,
  maxAgeMs: 30_000,
  kinds: ['device', 'settings']
});
