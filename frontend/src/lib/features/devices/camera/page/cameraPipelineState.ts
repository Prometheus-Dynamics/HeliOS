type LayoutParseResult = {
  rows: number;
  columns: number;
  slots: Record<string, string | null>;
  outputKeys: Record<string, string | null>;
};

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
  if (!layout || typeof layout !== 'object') return null;
  const raw = layout as any;
  const rows = Math.min(Math.max(Math.trunc(Number(raw?.rows ?? 1)), 1), 6);
  const columns = Math.min(Math.max(Math.trunc(Number(raw?.columns ?? 1)), 1), 6);
  const slots: Record<string, string | null> = {};
  const outputKeys: Record<string, string | null> = {};
  const entries = Array.isArray(raw?.slots) ? raw.slots : [];
  for (const entry of entries) {
    if (!entry || typeof entry !== 'object') continue;
    const row = Math.trunc(Number((entry as any).row ?? 0));
    const column = Math.trunc(Number((entry as any).column ?? 0));
    if (!Number.isFinite(row) || !Number.isFinite(column)) continue;
    if (row < 0 || column < 0) continue;
    if (row >= rows || column >= columns) continue;
    const pipelineIdRaw = (entry as any).pipeline_id ?? (entry as any).pipelineId ?? null;
    let pipelineId = typeof pipelineIdRaw === 'string' ? pipelineIdRaw.trim() : '';
    if (pipelineId === options.rawPipelineUuid) pipelineId = options.rawPipelineId;
    slots[`${row}:${column}`] = pipelineId.length ? pipelineId : null;
    const outputKeyRaw = (entry as any).output_key ?? (entry as any).outputKey ?? null;
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
