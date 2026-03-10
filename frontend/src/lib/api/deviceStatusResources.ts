import { apiFetch } from '$lib/api/core/http';
import { createDomainResource } from '$lib/api/domainResources';
import type { BootloaderStatus } from '$lib/ts-bindings/http/client';

export type ResourceGuardActionKind = 'disable_decoder' | 'disable_all_codecs' | 'stop_stream' | 'restore_codecs';

export type ResourceGuardAction = {
  at_ms: number;
  kind: ResourceGuardActionKind;
  stream_id: string;
  alias?: string | null;
  score: number;
  reason: string;
  mem_available_kb?: number | null;
};

export type ResourceGuardDegradedStream = {
  stream_id: string;
  alias?: string | null;
  stage: 'decoder_disabled' | 'codecs_disabled';
  changed_at_ms: number;
};

export type ResourceGuardStatus = {
  enabled: boolean;
  poll_ms: number;
  cooldown_ms: number;
  mem_low_kb: number;
  mem_recover_kb: number;
  last_mem_available_kb?: number | null;
  pressure_active: boolean;
  degraded_streams: ResourceGuardDegradedStream[];
  last_action?: ResourceGuardAction | null;
  recent_actions: ResourceGuardAction[];
};

const BOOTLOADER_STATUS_CACHE_KEY = 'device:bootloader:v1';
const RESOURCE_GUARD_STATUS_CACHE_KEY = 'device:resource-guard:v1';

export async function fetchBootloaderStatus(): Promise<BootloaderStatus> {
  return apiFetch<BootloaderStatus>('/device/bootloader');
}

export async function fetchResourceGuardStatus(): Promise<ResourceGuardStatus> {
  return apiFetch<ResourceGuardStatus>('/device/resource-guard');
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
