import { browser } from "$app/environment";
import { readable, type Readable } from "svelte/store";
import { getHttpClientBase } from "./httpClient";
import { buildWsUrl } from "$lib/api/wsClient";
import { connectionState, type ConnectionStatus } from "./connection";
import { createBackoffTimer } from "$lib/utils/backoff";

export type CpuCoreSample = {
  id: number;
  usage_percent: number;
  frequency_mhz: number | null;
  label: string | null;
};

export type CpuThrottleSample = {
  raw: number;
  undervoltage: boolean;
  frequency_capped: boolean;
  throttled: boolean;
  soft_temp_limit: boolean;
  undervoltage_since_boot: boolean;
  frequency_capped_since_boot: boolean;
  throttled_since_boot: boolean;
  soft_temp_limit_since_boot: boolean;
};

export type GpuMemorySample = {
  total_bytes: number;
  used_bytes: number;
  free_bytes: number;
};

export type DiskPartitionSample = {
  mount: string;
  total_bytes: number;
  used_bytes: number;
  free_bytes: number;
};

export type NetworkInterfaceSample = {
  name: string;
  rx_bytes: number;
  tx_bytes: number;
  total_rx_bytes: number;
  total_tx_bytes: number;
  rx_bytes_per_sec: number;
  tx_bytes_per_sec: number;
  mac: string | null;
};

export type NetworkSample = {
  rx_bytes: number;
  tx_bytes: number;
  total_rx_bytes: number;
  total_tx_bytes: number;
  rx_bytes_per_sec: number;
  tx_bytes_per_sec: number;
  interfaces: NetworkInterfaceSample[];
} | null;

export type EngineSample = {
  connected: boolean;
  last_disconnect_ms: number | null;
};

export type ResourceSample = {
  timestamp_ms: number;
  cpu: {
    usage_percent: number;
    temperature_c: number | null;
    throttle: CpuThrottleSample | null;
    cores: CpuCoreSample[];
  };
  memory: {
    total_bytes: number;
    used_bytes: number;
    free_bytes: number;
  };
  gpu: {
    usage_percent: number | null;
    temperature_c: number | null;
    frequency_mhz: number | null;
    memory: GpuMemorySample | null;
  } | null;
  disk: {
    total_bytes: number;
    used_bytes: number;
    free_bytes: number;
  } | null;
  disks: DiskPartitionSample[];
  power: {
    watts: number | null;
    volts: number | null;
    amps: number | null;
    raw?: unknown;
  } | null;
  network: NetworkSample;
  engine: EngineSample | null;
};

export const EMPTY_RESOURCE_SAMPLE: ResourceSample = {
  timestamp_ms: 0,
  cpu: { usage_percent: 0, temperature_c: null, throttle: null, cores: [] },
  memory: { total_bytes: 0, used_bytes: 0, free_bytes: 0 },
  gpu: null,
  disk: null,
  disks: [],
  power: null,
  network: null,
  engine: null,
};

const RECONNECT_DELAY_MS = 3000;
const RECONNECT_MAX_DELAY_MS = 30000;
const OFFLINE_RECONNECT_DELAY_MS = 15000;

export function createResourceTelemetryStore(): Readable<ResourceSample> {
  return readable(EMPTY_RESOURCE_SAMPLE, (set) => {
    if (!browser) {
      return () => {};
    }

    let socket: WebSocket | null = null;
    let closed = false;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let connectionStatus: ConnectionStatus = "unknown";
    const retryBackoff = createBackoffTimer({ baseMs: RECONNECT_DELAY_MS, maxMs: RECONNECT_MAX_DELAY_MS });

    function scheduleReconnect(delay?: number): void {
      if (closed || connectionStatus !== "online") return;
      if (retryTimer) clearTimeout(retryTimer);
      const wait = typeof delay === "number" ? delay : retryBackoff.getDelay();
      retryTimer = setTimeout(() => {
        retryTimer = null;
        connect();
      }, wait);
    }

    function connect(): void {
      if (closed) return;
      if (connectionStatus !== "online") {
        scheduleReconnect(OFFLINE_RECONNECT_DELAY_MS);
        return;
      }
      const url = buildTelemetryUrl();
      if (!url) {
        scheduleReconnect(OFFLINE_RECONNECT_DELAY_MS);
        return;
      }
      socket = new WebSocket(url);
      socket.addEventListener("open", () => {
        retryBackoff.reset();
      });
      socket.addEventListener("message", (event) => {
        const sample = parseSample(event.data);
        if (sample) {
          set(sample);
        }
      });
      socket.addEventListener("close", () => {
        if (closed) return;
        retryBackoff.bump();
        scheduleReconnect();
      });
      socket.addEventListener("error", () => {
        socket?.close();
      });
    }

    const unsubscribeConnection = connectionState.subscribe((snapshot) => {
      connectionStatus = snapshot.status;
      if (connectionStatus !== "online") {
        retryBackoff.reset();
        if (retryTimer) {
          clearTimeout(retryTimer);
          retryTimer = null;
        }
        socket?.close();
        socket = null;
      } else if (!socket && !closed) {
        retryBackoff.reset();
        scheduleReconnect(0);
      }
    });

    connect();

    return () => {
      closed = true;
      unsubscribeConnection();
      if (retryTimer) clearTimeout(retryTimer);
      socket?.close();
    };
  });
}

let sharedTelemetryStore: Readable<ResourceSample> | null = null;

/**
 * Use a shared telemetry stream so we only maintain a single websocket connection
 * even if multiple components subscribe to telemetry.
 */
export function getResourceTelemetryStore(): Readable<ResourceSample> {
  if (!sharedTelemetryStore) {
    sharedTelemetryStore = createResourceTelemetryStore();
  }
  return sharedTelemetryStore;
}

export const resourceTelemetryStore = getResourceTelemetryStore();

function buildTelemetryUrl(): string | null {
  const base = getHttpClientBase();
  const origin = browser ? globalThis.location?.origin?.trim() || "" : "";
  const url = buildWsUrl({
    base,
    path: ["v1", "ws", "telemetry"],
    allowRelative: false,
    fallbackOrigin: origin
  });
  try {
    new URL(url);
    return url;
  } catch {
    return null;
  }
}

function parseSample(payload: string): ResourceSample | null {
  try {
    const parsed = JSON.parse(payload);
    return normalizeResourceSample(parsed);
  } catch {
    return null;
  }
}

export function normalizeResourceSample(value: unknown): ResourceSample | null {
  if (typeof value !== "object" || value === null) return null;
  return normalizeSample(value);
}

function normalizeSample(value: unknown): ResourceSample {
  const sample = toRecord(value);
  const cpu = toOptionalRecord(sample["cpu"]);
  const memory = toOptionalRecord(sample["memory"]);
  const gpu = toOptionalRecord(sample["gpu"]);
  const disk = toOptionalRecord(sample["disk"]);
  const disks = normalizeDiskPartitions(sample["disks"]);
  const power = toOptionalRecord(sample["power"]);
  const engine = toOptionalRecord(sample["engine"]);

  return {
    timestamp_ms: toNumber(sample["timestamp_ms"]),
    cpu: {
      usage_percent: clampPercent(cpu?.["usage_percent"]),
      temperature_c: toOptionalNumber(cpu?.["temperature_c"]),
      throttle: normalizeCpuThrottle(cpu?.["throttle"]),
      cores: normalizeCpuCores(cpu?.["cores"]),
    },
    memory: {
      total_bytes: toUnsignedInteger(memory?.["total_bytes"]),
      used_bytes: toUnsignedInteger(memory?.["used_bytes"]),
      free_bytes: toUnsignedInteger(memory?.["free_bytes"]),
    },
    gpu: gpu
      ? {
          usage_percent: toOptionalNumber(gpu["usage_percent"]),
          temperature_c: toOptionalNumber(gpu["temperature_c"]),
          frequency_mhz: toOptionalNumber(gpu["frequency_mhz"]),
          memory: normalizeGpuMemory(gpu["memory"]),
        }
      : null,
    disk: disk
      ? {
          total_bytes: toUnsignedInteger(disk["total_bytes"]),
          used_bytes: toUnsignedInteger(disk["used_bytes"]),
          free_bytes: toUnsignedInteger(disk["free_bytes"]),
        }
      : null,
    disks,
    power: power
      ? {
          watts: toOptionalNumber(power["watts"]),
          volts: toOptionalNumber(power["volts"]),
          amps: toOptionalNumber(power["amps"]),
          raw: power["raw"] ?? undefined,
        }
      : null,
    network: normalizeNetwork(sample["network"]),
    engine: engine
      ? {
          connected: Boolean(engine["connected"]),
          last_disconnect_ms: toOptionalNumber(engine["last_disconnect_ms"]),
        }
      : null,
  };
}

function normalizeCpuThrottle(value: unknown): CpuThrottleSample | null {
  const record = toOptionalRecord(value);
  if (!record) return null;
  return {
    raw: toUnsignedInteger(record["raw"]),
    undervoltage: toBoolean(record["undervoltage"]),
    frequency_capped: toBoolean(record["frequency_capped"]),
    throttled: toBoolean(record["throttled"]),
    soft_temp_limit: toBoolean(record["soft_temp_limit"]),
    undervoltage_since_boot: toBoolean(record["undervoltage_since_boot"]),
    frequency_capped_since_boot: toBoolean(record["frequency_capped_since_boot"]),
    throttled_since_boot: toBoolean(record["throttled_since_boot"]),
    soft_temp_limit_since_boot: toBoolean(record["soft_temp_limit_since_boot"]),
  };
}

function normalizeCpuCores(value: unknown): CpuCoreSample[] {
  if (!Array.isArray(value)) return [];
  const cores: CpuCoreSample[] = [];
  for (const entry of value) {
    const record = toOptionalRecord(entry);
    if (!record) continue;
    const usage = clampPercent(record["usage_percent"]);
    const id = toUnsignedInteger(record["id"]);
    const label = toOptionalString(record["label"]);
    const frequency = toOptionalNumber(record["frequency_mhz"]);
    cores.push({
      id,
      usage_percent: usage,
      frequency_mhz: frequency && frequency > 0 ? frequency : null,
      label,
    });
  }
  return cores;
}

function normalizeGpuMemory(value: unknown): GpuMemorySample | null {
  const record = toOptionalRecord(value);
  if (!record) return null;
  const total = toUnsignedInteger(record["total_bytes"]);
  const used = toUnsignedInteger(record["used_bytes"]);
  const free = toUnsignedInteger(record["free_bytes"]);
  if (total === 0 && used === 0 && free === 0) return null;
  return { total_bytes: total, used_bytes: used, free_bytes: free };
}

function normalizeDiskPartitions(value: unknown): DiskPartitionSample[] {
  if (!Array.isArray(value)) return [];
  const partitions: DiskPartitionSample[] = [];
  for (const entry of value) {
    const record = toOptionalRecord(entry);
    if (!record) continue;
    const total = toUnsignedInteger(record["total_bytes"]);
    const used = toUnsignedInteger(record["used_bytes"]);
    const free = toUnsignedInteger(record["free_bytes"]);
    const mount = toOptionalString(record["mount"]) ?? "unknown";
    if (mount === "unknown" && total === 0 && used === 0 && free === 0) continue;
    partitions.push({ mount, total_bytes: total, used_bytes: used, free_bytes: free });
  }
  return partitions;
}

function normalizeNetworkInterfaces(value: unknown): NetworkInterfaceSample[] {
  if (!Array.isArray(value)) return [];
  const interfaces: NetworkInterfaceSample[] = [];
  for (const entry of value) {
    const record = toOptionalRecord(entry);
    if (!record) continue;
    interfaces.push({
      name: toOptionalString(record["name"]) ?? "unknown",
      rx_bytes: toUnsignedInteger(record["rx_bytes"]),
      tx_bytes: toUnsignedInteger(record["tx_bytes"]),
      total_rx_bytes: toUnsignedInteger(record["total_rx_bytes"]),
      total_tx_bytes: toUnsignedInteger(record["total_tx_bytes"]),
      rx_bytes_per_sec: toOptionalNumber(record["rx_bytes_per_sec"]) ?? 0,
      tx_bytes_per_sec: toOptionalNumber(record["tx_bytes_per_sec"]) ?? 0,
      mac: toOptionalString(record["mac"]),
    });
  }
  return interfaces;
}

function normalizeNetwork(value: unknown): NetworkSample {
  const record = toOptionalRecord(value);
  if (!record) return null;
  const rxBytes = toUnsignedInteger(record["rx_bytes"]);
  const txBytes = toUnsignedInteger(record["tx_bytes"]);
  const rxBytesPerSec = toOptionalNumber(record["rx_bytes_per_sec"]) ?? 0;
  const txBytesPerSec = toOptionalNumber(record["tx_bytes_per_sec"]) ?? 0;
  const totalRx = toUnsignedInteger(record["total_rx_bytes"]);
  const totalTx = toUnsignedInteger(record["total_tx_bytes"]);
  const interfaces = normalizeNetworkInterfaces(record["interfaces"]);
  if (
    rxBytes === 0 &&
    txBytes === 0 &&
    rxBytesPerSec === 0 &&
    txBytesPerSec === 0 &&
    totalRx === 0 &&
    totalTx === 0 &&
    interfaces.length === 0
  ) {
    return null;
  }
  return {
    rx_bytes: rxBytes,
    tx_bytes: txBytes,
    total_rx_bytes: totalRx,
    total_tx_bytes: totalTx,
    rx_bytes_per_sec: rxBytesPerSec,
    tx_bytes_per_sec: txBytesPerSec,
    interfaces,
  };
}

type JsonRecord = Record<string, unknown>;

function toRecord(value: unknown): JsonRecord {
  if (typeof value !== "object" || value === null) {
    return {};
  }
  return value as JsonRecord;
}

function toOptionalRecord(value: unknown): JsonRecord | null {
  if (typeof value !== "object" || value === null) {
    return null;
  }
  return value as JsonRecord;
}

function clampPercent(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, value));
}

function toOptionalNumber(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  return value;
}

function toOptionalString(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed.length ? trimmed : null;
}

function toBoolean(value: unknown): boolean {
  return value === true;
}

function toUnsignedInteger(value: unknown): number {
  const numeric = toNumber(value);
  if (!Number.isFinite(numeric) || numeric < 0) {
    return 0;
  }
  return Math.floor(numeric);
}

function toNumber(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return 0;
  }
  return value;
}
