import { get, writable, type Readable } from 'svelte/store';
import { apiFetch } from './api';
import { buildErrorMessage } from '$lib/ui/errorPolicy';
import { invalidateSWR, readSWR, primeSWR } from '$lib/utils/swrCache';
import type { DeviceNetworkInterfaceView, DeviceSettingsData, DeviceSettingsPatchRequest, FanConfig, FanStatus, LedConfig, Nt4Settings, OsReleaseInfo, UsbPowerSettings } from './types';
import type { HostnamePayload, NetworkInterfaceSettings, TeamNumberPayload } from '$lib/api/client';

export type DeviceSettingsState = {
  data: DeviceSettingsData | null;
  loading: boolean;
  error: string | null;
  revision: string | null;
  initialized: boolean;
};

const SETTINGS_CACHE_KEY = 'settings:device:v1';
const SETTINGS_CACHE_STALE_MS = 10_000;
const SETTINGS_CACHE_MAX_MS = 120_000;

const initialState: DeviceSettingsState = {
  data: null,
  loading: true,
  error: null,
  revision: null,
  initialized: false
};

const store = writable<DeviceSettingsState>(initialState);
let loadNonce = 0;

const DEFAULT_DIAGNOSTICS = { keep: 0, max_mb: 0, tar: false };

function buildSettingsBase(current: DeviceSettingsData | null): DeviceSettingsData {
  return {
    hostname: current?.hostname ?? 'unknown',
    team_number: typeof current?.team_number === 'number' ? current.team_number : null,
    interfaces: Array.isArray(current?.interfaces) ? current.interfaces : [],
    os_release: current?.os_release ?? null,
    nt4: current?.nt4 ?? null,
    lighting: current?.lighting ?? null,
    usb_power: current?.usb_power ?? null,
    fan: current?.fan ?? null,
    fan_status: current?.fan_status ?? null,
    diagnostics: current?.diagnostics ?? DEFAULT_DIAGNOSTICS,
    revision: current?.revision ?? ''
  };
}

type DeviceSettingsStore = Readable<DeviceSettingsState> & {
  load: typeof loadDeviceSettings;
  patch: typeof patchDeviceSettings;
};

export const deviceSettingsStore: DeviceSettingsStore = {
  subscribe: store.subscribe,
  load: loadDeviceSettings,
  patch: patchDeviceSettings
};

function isIpv4(value: string): boolean {
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(value.trim());
}

function parseAssignment(raw: unknown): { address: string; prefix: number } | null {
  if (!raw) return null;
  if (typeof raw === 'string') {
    const [address, prefixRaw] = raw.split('/');
    const prefix = Number(prefixRaw);
    if (!address || !Number.isFinite(prefix)) return null;
    return { address: address.trim(), prefix: prefix };
  }
  if (typeof raw === 'object') {
    const entry = raw as { address?: unknown; prefix?: unknown };
    const address = typeof entry.address === 'string' ? entry.address.trim() : '';
    const prefix = typeof entry.prefix === 'number' ? entry.prefix : Number(entry.prefix);
    if (!address || !Number.isFinite(prefix)) return null;
    return { address, prefix };
  }
  return null;
}

function prefixToNetmask(prefix: number): string {
  const normalized = Math.max(0, Math.min(32, Math.trunc(prefix)));
  const mask = normalized === 0 ? 0 : (0xffffffff << (32 - normalized)) >>> 0;
  return `${(mask >>> 24) & 255}.${(mask >>> 16) & 255}.${(mask >>> 8) & 255}.${mask & 255}`;
}

function mapInterfaces(raw: NetworkInterfaceSettings[]): DeviceNetworkInterfaceView[] {
  return raw
    .filter((iface) => typeof iface.name === 'string' && iface.name.trim().length)
    .map((iface) => {
      const mode = iface.mode === 'Static' ? 'static' : 'dhcp';
      const mac = typeof iface.mac === 'string' ? iface.mac : null;
      const assignments = Array.isArray(iface.ipv4) ? iface.ipv4 : [];
      const firstV4 = assignments.map(parseAssignment).find((entry) => entry && isIpv4(entry.address)) ?? null;
      const gateways = Array.isArray(iface.gateways) ? iface.gateways : [];
      const gateway =
        (typeof iface.gateway === 'string' && iface.gateway.trim().length ? iface.gateway.trim() : null) ??
        gateways.find((entry) => typeof entry === 'string' && isIpv4(entry)) ??
        null;

      if (mode === 'static') {
        return {
          name: iface.name!.trim(),
          mode,
          mac,
          static_ipv4: firstV4
            ? {
                address: firstV4.address,
                prefix: firstV4.prefix,
                gateway
              }
            : null,
          dhcp_ipv4: null
        };
      }

      return {
        name: iface.name!.trim(),
        mode,
        mac,
        static_ipv4: null,
        dhcp_ipv4: firstV4
          ? {
              address: firstV4.address,
              prefix: firstV4.prefix,
              gateway
            }
          : null
      };
    });
}

async function loadDeviceSettings(options: { quiet?: boolean; force?: boolean } = {}): Promise<void> {
  const { quiet = false, force = false } = options;
  const cached = force
    ? null
    : readSWR<DeviceSettingsData>(SETTINGS_CACHE_KEY, {
        staleMs: SETTINGS_CACHE_STALE_MS,
        maxAgeMs: SETTINGS_CACHE_MAX_MS
      });
  const requestId = ++loadNonce;
  if (force) {
    invalidateSWR(SETTINGS_CACHE_KEY);
  }
  if (!quiet) {
    store.update((state) => ({ ...state, loading: true, error: null }));
  }
  try {
    if (cached?.data && (!quiet || !get(store).data)) {
      store.update((state) => ({
        ...state,
        data: cached.data,
        loading: false,
        error: null,
        revision: cached.data.revision ?? null,
        initialized: true
      }));
    }
    const base = buildSettingsBase(get(store).data ?? cached?.data ?? null);
    const [interfacesResult, hostnameResult, teamResult] = await Promise.allSettled([
      apiFetch<NetworkInterfaceSettings[]>('/device/network'),
      apiFetch<HostnamePayload>('/device/hostname'),
      apiFetch<TeamNumberPayload>('/device/team')
    ]);

    if (requestId !== loadNonce) return;

    const failures: unknown[] = [];
    const partial: Partial<DeviceSettingsData> = {};

    if (interfacesResult.status === 'fulfilled') {
      partial.interfaces = mapInterfaces(interfacesResult.value);
    } else {
      failures.push(interfacesResult.reason);
    }
    if (hostnameResult.status === 'fulfilled') {
      partial.hostname = hostnameResult.value.hostname;
    } else {
      failures.push(hostnameResult.reason);
    }
    if (teamResult.status === 'fulfilled') {
      partial.team_number = typeof teamResult.value.team_number === 'number' ? teamResult.value.team_number : null;
    } else {
      failures.push(teamResult.reason);
    }

    const payload = { ...base, ...partial };
    const message = failures.length
      ? buildErrorMessage({ error: failures[0], fallback: 'Unable to load device settings.' })
      : null;
    const missingCore = failures.length === 3 && !cached?.data && !get(store).data;

    if (missingCore) {
      invalidateSWR(SETTINGS_CACHE_KEY);
      store.set({
        data: null,
        loading: false,
        error: message,
        revision: null,
        initialized: true
      });
      throw new Error(message ?? 'Unable to load device settings.');
    }

    primeSWR(SETTINGS_CACHE_KEY, payload);
    store.set({
      data: payload,
      loading: false,
      error: failures.length ? message : null,
      revision: payload.revision,
      initialized: true
    });

    const applyOptional = (update: Partial<DeviceSettingsData>) => {
      store.update((state) => {
        if (requestId !== loadNonce) return state;
        const next = { ...buildSettingsBase(state.data ?? null), ...update };
        primeSWR(SETTINGS_CACHE_KEY, next);
        return { ...state, data: next, revision: next.revision ?? state.revision ?? null, initialized: true };
      });
    };

    void apiFetch<Nt4Settings>('/device/nt4')
      .then((nt4) => applyOptional({ nt4: nt4 ?? null }))
      .catch(() => applyOptional({ nt4: null }));
    void apiFetch<OsReleaseInfo>('/device/os')
      .then((osRelease) => applyOptional({ os_release: osRelease ?? null }))
      .catch(() => applyOptional({ os_release: null }));
    void apiFetch<LedConfig>('/device/lighting/config')
      .then((lighting) => applyOptional({ lighting: lighting ?? null }))
      .catch(() => applyOptional({ lighting: null }));
    void apiFetch<UsbPowerSettings>('/device/usb-power')
      .then((usbPower) => applyOptional({ usb_power: usbPower ?? null }))
      .catch(() => applyOptional({ usb_power: null }));
    void apiFetch<FanConfig>('/device/fan/config')
      .then((fan) => applyOptional({ fan: fan ?? null }))
      .catch(() => applyOptional({ fan: null }));
    void apiFetch<FanStatus>('/peripherals/fan')
      .then((fanStatus) => applyOptional({ fan_status: fanStatus ?? null }))
      .catch(() => applyOptional({ fan_status: null }));
  } catch (err) {
    if (requestId !== loadNonce) return;
    const message = buildErrorMessage({ error: err, fallback: 'Unable to load device settings.' });
    invalidateSWR(SETTINGS_CACHE_KEY);
    store.set({
      data: null,
      loading: false,
      error: message,
      revision: null,
      initialized: true
    });
    throw err;
  }
}

async function patchDeviceSettings(request: DeviceSettingsPatchRequest): Promise<void> {
  const operations: Promise<unknown>[] = [];
  const optimistic: Partial<DeviceSettingsData> = {};

  if (typeof request.hostname === 'string') {
    operations.push(
      apiFetch<void>('/device/hostname', {
        method: 'POST',
        body: { hostname: request.hostname }
      })
    );
    optimistic.hostname = request.hostname;
  }

  if (typeof request.team_number !== 'undefined') {
    operations.push(
      apiFetch<void>('/device/team', {
        method: 'POST',
        body: { team_number: request.team_number }
      })
    );
    optimistic.team_number = typeof request.team_number === 'number' ? request.team_number : null;
  }

  if (Array.isArray(request.interfaces)) {
    for (const entry of request.interfaces) {
      if (!entry?.name) continue;
      if (entry.assignment?.mode === 'dhcp') {
        operations.push(
          apiFetch<void>('/device/network', {
            method: 'POST',
            body: { name: entry.name, mode: 'Dynamic' }
          })
        );
        continue;
      }
      if (entry.assignment?.mode === 'static') {
        const gateway = entry.assignment.gateway?.trim() || undefined;
        const address = entry.assignment.address.trim();
        const prefix = entry.assignment.prefix;
        operations.push(
          apiFetch<void>('/device/network', {
            method: 'POST',
            body: {
              name: entry.name,
              mode: 'Static',
              ipv4: [{ address, prefix }],
              address,
              netmask: prefixToNetmask(prefix),
              gateway: gateway ?? null,
              gateways: gateway ? [gateway] : []
            }
          })
        );
      }
    }
  }

  if (request.nt4) {
    operations.push(
      apiFetch<void>('/device/nt4', {
        method: 'POST',
        body: request.nt4
      })
    );
    optimistic.nt4 = request.nt4;
  }

  if (request.lighting) {
    operations.push(
      apiFetch<void>('/device/lighting/config', {
        method: 'POST',
        body: { lighting: request.lighting, requested_by: request.requested_by }
      })
    );
    optimistic.lighting = request.lighting;
  }

  if (request.usb_power) {
    operations.push(
      apiFetch<void>('/device/usb-power', {
        method: 'POST',
        body: request.usb_power
      })
    );
    optimistic.usb_power = request.usb_power;
  }

  if (request.fan) {
    operations.push(
      apiFetch<void>('/device/fan/config', {
        method: 'POST',
        body: { fan: request.fan, requested_by: request.requested_by }
      })
    );
    optimistic.fan = request.fan;
  }

  await Promise.all(operations);
  if (Object.keys(optimistic).length > 0) {
    store.update((state) => {
      const next = { ...buildSettingsBase(state.data ?? null), ...optimistic };
      primeSWR(SETTINGS_CACHE_KEY, next);
      return { ...state, data: next, revision: next.revision ?? state.revision ?? null, initialized: true };
    });
  }
  await loadDeviceSettings({ quiet: true, force: true });
}
