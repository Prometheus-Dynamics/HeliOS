import type { ControlKind, ControlMeta, ControlValue } from '$lib/api/client';

export type ControlValueWire = { kind?: string; value?: unknown };

export function extractControlValue(value: ControlValue | ControlValueWire | null | undefined): number | boolean | null {
  if (!value || value === 'None') return null;
  if (typeof value === 'object') {
    if ('Bool' in value) return value.Bool;
    if ('Int' in value) return value.Int;
    if ('Uint' in value) return value.Uint;
    if ('Float' in value) return value.Float;

    const kind = typeof (value as ControlValueWire).kind === 'string' ? (value as ControlValueWire).kind : null;
    const payload = (value as ControlValueWire).value;
    if (!kind) return null;
    if (kind === 'none') return null;
    if (kind === 'bool') return Boolean(payload);
    if (kind === 'int' || kind === 'int32' || kind === 'int64') return Number(payload);
    if (kind === 'uint' || kind === 'uint32' || kind === 'uint16' || kind === 'byte')
      return Math.max(0, Number(payload));
    if (kind === 'float') return Number(payload);
  }
  return null;
}

export function seedControlState(entries: ControlMeta[]): Record<number, number | boolean | null> {
  const map: Record<number, number | boolean | null> = {};
  for (const ctrl of entries) {
    const currentValue =
      'value' in ctrl ? (ctrl as { value?: ControlValue | ControlValueWire | null }).value : undefined;
    const current = extractControlValue(currentValue ?? ctrl.default);
    map[ctrl.id] = current;
  }
  return map;
}

export function accessLabel(access: ControlMeta['access'] | string | null | undefined): string {
  switch (access) {
    case 'ReadWrite':
      return 'Read|Write';
    case 'ReadOnly':
      return 'Read';
    default:
      return String(access ?? '');
  }
}

export function accessBadgeClass(access: ControlMeta['access'] | string | null | undefined): string {
  switch (access) {
    case 'ReadWrite':
      return 'border-primary-500/50 bg-primary-500/10 text-primary-100';
    case 'ReadOnly':
      return 'border-surface-700/70 bg-surface-900/40 text-surface-400';
    default:
      return 'border-surface-700/70 bg-surface-900/40 text-surface-400';
  }
}

export function menuOptions(ctrl: ControlMeta): Array<{ value: number; label: string }> {
  const list = Array.isArray(ctrl.menu) ? ctrl.menu : [];
  const options: Array<{ value: number; label: string }> = [];
  list.forEach((entry: unknown, idx: number) => {
    if (typeof entry === 'number') {
      options.push({ value: entry, label: String(entry) });
    } else if (typeof entry === 'string') {
      options.push({ value: idx, label: entry });
    } else if (entry && typeof entry === 'object') {
      const record = entry as Record<string, unknown>;
      const val = Number(record.value ?? record.id ?? idx);
      const label = String(record.label ?? record.name ?? val);
      options.push({ value: Number.isFinite(val) ? val : idx, label });
    } else {
      options.push({ value: idx, label: `Option ${idx + 1}` });
    }
  });
  return options.length ? options : [{ value: 0, label: '0' }];
}

export function menuValueDisplay(ctrl: ControlMeta, value: number | boolean | null): string {
  const numeric = Math.trunc(Number(value ?? 0));
  if (!Number.isFinite(numeric)) return 'n/a';
  const options = menuOptions(ctrl);
  const found = options.find((opt) => opt.value === numeric);
  if (!found) return String(numeric);
  const label = String(found.label ?? '').trim();
  const num = String(numeric);
  if (!label.length || label === num) return `[${num}]`;
  return `[${num}] - ${label}`;
}

export function displayControlValue(ctrl: ControlMeta, value: number | boolean | null): string {
  if (value == null) return '—';
  const kind = ctrl.kind;
  if (kind === 'Bool') return value ? 'On' : 'Off';
  if (kind === 'Menu' || kind === 'IntMenu') return menuValueDisplay(ctrl, value);
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) return '—';
    if (kind === 'Float') return value.toFixed(3).replace(/\.?0+$/, '');
    return String(Math.trunc(value));
  }
  return String(value);
}

export function controlMin(ctrl: ControlMeta): number | null {
  const value = extractControlValue(ctrl.min);
  return typeof value === 'number' ? value : null;
}

export function controlMax(ctrl: ControlMeta): number | null {
  const value = extractControlValue(ctrl.max);
  return typeof value === 'number' ? value : null;
}

export function controlStep(ctrl: ControlMeta): number | null {
  const step = ctrl.step ? extractControlValue(ctrl.step) : null;
  return typeof step === 'number' && Number.isFinite(step) && step > 0 ? step : null;
}

export const clampNumber = (value: number, min: number | null, max: number | null): number => {
  let next = value;
  if (min != null) next = Math.max(min, next);
  if (max != null) next = Math.min(max, next);
  return next;
};

export function clampControlValue(ctrl: ControlMeta, next: number | boolean | null): number | boolean | null {
  if (next == null) return null;
  const kind = ctrl.kind;
  if (kind === 'Bool') return Boolean(next);
  if (kind === 'Menu' || kind === 'IntMenu') {
    const options = menuOptions(ctrl);
    const numeric = Math.trunc(Number(next ?? 0));
    if (!Number.isFinite(numeric)) return null;
    const allowed = new Set(options.map((o) => o.value));
    if (allowed.has(numeric)) return numeric;
    const min = Math.min(...options.map((o) => o.value));
    const max = Math.max(...options.map((o) => o.value));
    return clampNumber(numeric, min, max);
  }
  const numeric = Number(next);
  if (!Number.isFinite(numeric)) return null;
  const min = controlMin(ctrl);
  const max = controlMax(ctrl);
  const clamped = clampNumber(numeric, min, max);
  return kind === 'Float' ? clamped : Math.trunc(clamped);
}

export function buildControlValue(kind: ControlKind, next: number | boolean | null): ControlValue {
  if (next == null) return 'None';
  switch (kind) {
    case 'Bool':
      return { Bool: Boolean(next) };
    case 'Int':
      return { Int: Math.trunc(Number(next ?? 0)) };
    case 'Uint': {
      const v = Math.max(0, Number(next ?? 0));
      return { Uint: Math.trunc(v) };
    }
    case 'Float':
      return { Float: Number(next ?? 0) };
    case 'Menu':
      return { Uint: Math.max(0, Math.trunc(Number(next ?? 0))) };
    case 'IntMenu':
      return { Int: Math.trunc(Number(next ?? 0)) };
    default:
      return 'None';
  }
}
