type LayoutParseResult = {
  rows: number;
  columns: number;
  slots: Record<string, string | null>;
  outputKeys: Record<string, string | null>;
};

type ManifestLayoutEntry = {
  row?: unknown;
  column?: unknown;
  pipeline_id?: unknown;
  output_key?: unknown;
};

function asRecord<T extends Record<string, unknown>>(value: unknown): T | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as T;
}

export function normalizeGridSlots(
  rows: number,
  columns: number,
  slots: Record<string, string | null>
): Record<string, string | null> {
  const next: Record<string, string | null> = {};
  Object.entries(slots ?? {}).forEach(([key, value]) => {
    const [rRaw, cRaw] = key.split(':');
    const row = Number(rRaw);
    const column = Number(cRaw);
    if (!Number.isInteger(row) || !Number.isInteger(column)) return;
    if (row < 0 || column < 0) return;
    if (row >= rows || column >= columns) return;
    const v = typeof value === 'string' ? value.trim() : '';
    next[`${row}:${column}`] = v.length ? v : null;
  });
  return next;
}

export function normalizeGridOutputKeys(
  rows: number,
  columns: number,
  keys: Record<string, string | null>
): Record<string, string | null> {
  const next: Record<string, string | null> = {};
  Object.entries(keys ?? {}).forEach(([key, value]) => {
    const [rRaw, cRaw] = key.split(':');
    const row = Number(rRaw);
    const column = Number(cRaw);
    if (!Number.isInteger(row) || !Number.isInteger(column)) return;
    if (row < 0 || column < 0) return;
    if (row >= rows || column >= columns) return;
    const v = typeof value === 'string' ? value.trim() : '';
    next[`${row}:${column}`] = v.length ? v : null;
  });
  return next;
}

export function layoutSignature(
  rows: number,
  columns: number,
  slots: Record<string, string | null>,
  outputKeys: Record<string, string | null>
): string {
  const safeRows = Math.min(Math.max(Math.trunc(rows), 1), 6);
  const safeColumns = Math.min(Math.max(Math.trunc(columns), 1), 6);
  const normalizedSlots = normalizeGridSlots(safeRows, safeColumns, slots);
  const normalizedOutputs = normalizeGridOutputKeys(safeRows, safeColumns, outputKeys);
  const keys = Array.from(new Set([...Object.keys(normalizedSlots), ...Object.keys(normalizedOutputs)])).sort();
  const entries = keys.map((key) => [key, normalizedSlots[key] ?? null, normalizedOutputs[key] ?? null]);
  return JSON.stringify({ rows: safeRows, columns: safeColumns, entries });
}

export function parseManifestLayout(
  layout: unknown,
  options: { rawPipelineId: string; rawPipelineUuid: string }
): LayoutParseResult | null {
  const raw = asRecord<{ rows?: unknown; columns?: unknown; slots?: unknown }>(layout);
  if (!raw) return null;
  const rows = Math.min(Math.max(Math.trunc(Number(raw.rows ?? 1)), 1), 6);
  const columns = Math.min(Math.max(Math.trunc(Number(raw.columns ?? 1)), 1), 6);
  const slots: Record<string, string | null> = {};
  const outputKeys: Record<string, string | null> = {};
  const entries = Array.isArray(raw.slots) ? raw.slots : [];
  for (const entryValue of entries) {
    const entry = asRecord<ManifestLayoutEntry>(entryValue);
    if (!entry) continue;
    const row = Math.trunc(Number(entry.row ?? 0));
    const column = Math.trunc(Number(entry.column ?? 0));
    if (!Number.isFinite(row) || !Number.isFinite(column)) continue;
    if (row < 0 || column < 0) continue;
    if (row >= rows || column >= columns) continue;
    const pipelineIdRaw = entry.pipeline_id ?? null;
    let pipelineId = typeof pipelineIdRaw === 'string' ? pipelineIdRaw.trim() : '';
    if (pipelineId === options.rawPipelineUuid) pipelineId = options.rawPipelineId;
    slots[`${row}:${column}`] = pipelineId.length ? pipelineId : null;
    const outputKeyRaw = entry.output_key ?? null;
    let outputKey = typeof outputKeyRaw === 'string' ? outputKeyRaw.trim() : '';
    if (pipelineId === options.rawPipelineId && outputKey.toLowerCase() === 'frame') {
      outputKey = 'raw';
    }
    outputKeys[`${row}:${column}`] = outputKey.length ? outputKey : null;
  }
  return {
    rows,
    columns,
    slots: normalizeGridSlots(rows, columns, slots),
    outputKeys: normalizeGridOutputKeys(rows, columns, outputKeys)
  };
}
