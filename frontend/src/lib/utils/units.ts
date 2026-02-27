export type LengthUnit = 'm' | 'cm' | 'mm' | 'in' | 'ft';

const UNIT_TO_METERS: Record<LengthUnit, number> = {
  m: 1,
  cm: 0.01,
  mm: 0.001,
  in: 0.0254,
  ft: 0.3048
};

function normalizeUnit(unitRaw: string): LengthUnit | null {
  const normalized = unitRaw.trim().toLowerCase().replaceAll('_', '');
  const unit = normalized.startsWith('/') ? normalized.slice(1) : normalized;

  if (!unit) return null;
  if (unit === 'm' || unit === 'meter' || unit === 'meters') return 'm';
  if (unit === 'cm' || unit === 'centimeter' || unit === 'centimeters') return 'cm';
  if (unit === 'mm' || unit === 'millimeter' || unit === 'millimeters') return 'mm';
  if (unit === 'in' || unit === 'inch' || unit === 'inches' || unit === '"') return 'in';
  if (unit === 'ft' || unit === 'feet' || unit === 'foot' || unit === "'") return 'ft';
  return null;
}

function parseNumberOrFraction(input: string): number | null {
  const value = input.trim();
  if (!value) return null;

  if (value.includes('/')) {
    const [numeratorRaw, denominatorRaw, ...rest] = value.split('/');
    if (rest.length) return null;
    const numerator = Number(numeratorRaw);
    const denominator = Number(denominatorRaw);
    if (!Number.isFinite(numerator) || !Number.isFinite(denominator) || Math.abs(denominator) < Number.EPSILON) {
      return null;
    }
    const frac = numerator / denominator;
    return Number.isFinite(frac) ? frac : null;
  }

  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export type ParsedLength = {
  meters: number;
  unit: LengthUnit;
  magnitude: number;
};

export type LengthValue = ParsedLength;

export function parseLengthToMeters(input: string, defaultUnit: LengthUnit = 'm'): ParsedLength | null {
  const raw = input.trim();
  if (!raw) return null;

  const match = raw.match(/^([+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:\/\d+(?:\.\d+)?)?)\s*([a-zA-Z"']+)?$/);
  if (!match) return null;

  const magnitude = parseNumberOrFraction(match[1] ?? '');
  if (magnitude === null) return null;

  const unit = match[2] ? normalizeUnit(match[2]) : defaultUnit;
  if (!unit) return null;

  const meters = magnitude * UNIT_TO_METERS[unit];
  if (!Number.isFinite(meters)) return null;

  return { meters, unit, magnitude };
}

export function formatMeters(valueMeters: number, unit: LengthUnit, fractionDigits = 3): string {
  const factor = UNIT_TO_METERS[unit];
  const magnitude = valueMeters / factor;
  if (!Number.isFinite(magnitude)) return '—';
  return `${magnitude.toFixed(fractionDigits)} ${unit}`;
}
