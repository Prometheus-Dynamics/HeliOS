import type { PipelineDataType, PipelineTypeDescriptor } from '$lib/types/pipeline';

const DEFAULT_TYPE_OPTIONS: Array<{ key: string; label: string }> = [
  { key: 'generic', label: 'Generic' },
  { key: 'image:dynamic', label: 'Image (dynamic)' },
  { key: 'json', label: 'Json' },
  { key: 'string', label: 'String' },
  { key: 'int', label: 'Int' },
  { key: 'float', label: 'Float' },
  { key: 'bool', label: 'Bool' }
];

const isTypeDescriptor = (value: unknown): value is PipelineTypeDescriptor =>
  typeof value === 'object' && value !== null && 'id' in value;

export const presentTypeString = (raw: string): string => {
  const trimmed = raw.trim();
  if (!trimmed) return 'Unknown';
  const imageFlavor = trimmed.match(/^image:(.+)$/i)?.[1]?.trim() ?? null;
  if (imageFlavor) return `Image (${imageFlavor})`;
  if (/^enum\s*</iu.test(trimmed)) return 'Enum';
  if (trimmed.toLowerCase() === 'json') return 'Json';
  return trimmed;
};

export const describePortType = (
  value: PipelineTypeDescriptor | PipelineDataType | string | undefined,
  typePalette: Record<string, PipelineTypeDescriptor> = {}
): string => {
  if (!value) return 'Unknown';
  if (typeof value === 'string') return presentTypeString(value);
  if (isTypeDescriptor(value)) {
    const paletteEntry = typePalette[value.id];
    const label = value.label ?? paletteEntry?.label ?? paletteEntry?.id ?? null;
    return label ?? value.id ?? 'Unknown';
  }
  const descriptor = value.descriptor ?? (value.kind ? typePalette[value.kind] : undefined);
  const label = value.label ?? descriptor?.label ?? descriptor?.id ?? null;
  const kind = value.kind ?? descriptor?.id ?? null;
  if (label && kind && label !== kind) return `${label} (${kind})`;
  return label ?? kind ?? 'Unknown';
};

export const buildTypeOptions = (typePalette: Record<string, PipelineTypeDescriptor> = {}): Array<{ key: string; label: string }> => {
  const seen = new Set<string>();
  const options: Array<{ key: string; label: string }> = [];
  for (const entry of DEFAULT_TYPE_OPTIONS) {
    if (seen.has(entry.key)) continue;
    seen.add(entry.key);
    options.push(entry);
  }
  for (const [id, descriptor] of Object.entries(typePalette)) {
    const key = String(id ?? '').trim();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    const label = String(descriptor?.label ?? key).trim() || key;
    options.push({ key, label });
  }
  return options.sort((a, b) => a.label.localeCompare(b.label));
};
