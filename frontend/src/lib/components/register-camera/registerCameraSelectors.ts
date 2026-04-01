import type { CodecInfo, Interval, Mode, ProbedBackend, ProbedDevice, StreamInfo } from '$lib/ts-bindings/http/client';
import { encoderSelectionId } from '$lib/api/streamEncoderSettings';

export function currentDevice(devices: ProbedDevice[], selectedDeviceIndex: number): ProbedDevice | null {
  return selectedDeviceIndex >= 0 ? devices[selectedDeviceIndex] ?? null : null;
}

export function currentBackend(
  devices: ProbedDevice[],
  selectedDeviceIndex: number,
  selectedBackendIndex: number
): ProbedBackend | null {
  return currentDevice(devices, selectedDeviceIndex)?.backends?.[selectedBackendIndex] ?? null;
}

export function currentModes(
  devices: ProbedDevice[],
  selectedDeviceIndex: number,
  selectedBackendIndex: number
): Mode[] {
  return currentBackend(devices, selectedDeviceIndex, selectedBackendIndex)?.descriptor?.modes ?? [];
}

export function resolutionKey(mode: Mode | undefined): string | null {
  const res = mode?.format?.resolution;
  if (!res?.width || !res?.height) return null;
  return `${res.width}x${res.height}`;
}

export function formatLabel(fmt: string | null | undefined): string {
  if (!fmt) return 'Unspecified';
  return fmt.toUpperCase();
}

export function formats(
  devices: ProbedDevice[],
  selectedDeviceIndex: number,
  selectedBackendIndex: number
): string[] {
  const seen = new Set<string>();
  const list: string[] = [];
  for (const mode of currentModes(devices, selectedDeviceIndex, selectedBackendIndex)) {
    const fmt = formatLabel(mode.format?.code);
    if (seen.has(fmt)) continue;
    seen.add(fmt);
    list.push(fmt);
  }
  return list;
}

export function resolutionsForSelectedFormat(
  devices: ProbedDevice[],
  selectedDeviceIndex: number,
  selectedBackendIndex: number,
  selectedFormat: string | null
): Mode[] {
  const fmt = selectedFormat;
  const seen = new Set<string>();
  const result: Mode[] = [];
  for (const mode of currentModes(devices, selectedDeviceIndex, selectedBackendIndex)) {
    if (formatLabel(mode.format?.code) !== fmt) continue;
    const key = resolutionKey(mode);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    result.push(mode);
  }
  return result;
}

export function decodersForFormat(decoders: CodecInfo[], selectedFormat: string | null): CodecInfo[] {
  const fmt = selectedFormat;
  if (!fmt) return decoders;
  const matches = decoders.filter((d) => {
    const code = formatLabel(d.input || d.fourcc);
    return code === fmt || code === 'ANY';
  });
  const seen = new Set<string>();
  const result: CodecInfo[] = [];
  for (const decoder of matches) {
    const key = `${decoder.implementation ?? ''}|${decoder.input ?? decoder.fourcc ?? ''}`;
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(decoder);
  }
  return result;
}

export function intervalsForSelected(
  devices: ProbedDevice[],
  selectedDeviceIndex: number,
  selectedBackendIndex: number,
  selectedResolutionKey: string | null,
  selectedFormat: string | null
): Interval[] {
  const key = selectedResolutionKey;
  const fmt = selectedFormat;
  const modes = currentModes(devices, selectedDeviceIndex, selectedBackendIndex);
  const mode =
    modes.find((m) => resolutionKey(m) === key && formatLabel(m.format?.code) === fmt) ??
    modes.find((m) => formatLabel(m.format?.code) === fmt) ??
    modes[0];
  return (mode?.intervals ?? []) as Interval[];
}

export function currentEncoder(encoders: CodecInfo[], encoderImpl: string | null): CodecInfo | undefined {
  if (!encoderImpl) return undefined;
  const selected = encoderImpl.trim();
  if (!selected) return undefined;
  return encoders.find((c) => encoderSelectionId(c) === selected || c.implementation === selected || c.name === selected);
}

export function intervalToFps(interval: Interval | null | undefined): string | null {
  if (!interval?.numerator || !interval?.denominator) return null;
  const fps = interval.denominator / interval.numerator;
  if (!Number.isFinite(fps) || fps <= 0) return null;
  return fps.toFixed(2).replace(/\.00$/, '');
}

export function resolutionLabel(mode: Mode | undefined): string {
  const res = mode?.format?.resolution;
  return res?.width && res?.height ? `${res.width}x${res.height}` : 'Unknown res';
}

export function isRegistered(device: ProbedDevice | null, registeredHardwareIds: string[]): boolean {
  if (!device) return false;
  const deviceKey = device.identity?.keys?.[0];
  return Boolean(deviceKey && registeredHardwareIds.includes(deviceKey));
}

export function streamAssignedKeys(streams: StreamInfo[]): Set<string> {
  const used = new Set<string>();
  for (const stream of streams ?? []) {
    const manifest = stream?.manifest;
    const keys = (manifest?.identity as { keys?: unknown } | null | undefined)?.keys;
    if (Array.isArray(keys)) {
      for (const key of keys) {
        if (typeof key === 'string' && key.trim().length) {
          used.add(key.trim());
        }
      }
    }
    for (const key of manifest?.capture?.device_keys ?? []) {
      if (typeof key === 'string' && key.trim().length) {
        used.add(key.trim());
      }
    }
  }
  return used;
}

export function deviceIsInUse(device: ProbedDevice, usedKeys: Set<string>): boolean {
  const keys = device?.identity?.keys ?? [];
  return keys.some((key) => typeof key === 'string' && usedKeys.has(key.trim()));
}
