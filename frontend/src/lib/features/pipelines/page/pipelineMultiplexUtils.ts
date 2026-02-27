export const multiplexKey = (row: number, column: number): string => `${row}:${column}`;

export const normalizeMultiplexSlots = (
  rows: number,
  columns: number,
  slots: Record<string, string | null>
): Record<string, string | null> => {
  const next: Record<string, string | null> = {};
  for (let row = 0; row < rows; row += 1) {
    for (let column = 0; column < columns; column += 1) {
      const key = multiplexKey(row, column);
      next[key] = Object.prototype.hasOwnProperty.call(slots, key) ? slots[key] ?? null : null;
    }
  }
  return next;
};

export const buildTuneMultiplexSignature = (
  rows: number,
  columns: number,
  slots: Record<string, string | null>,
  outputs: Record<string, string | null>
): string => {
  const normalizedSlots = normalizeMultiplexSlots(rows, columns, slots);
  const normalizedOutputs = normalizeMultiplexSlots(rows, columns, outputs);
  const entries = Object.keys(normalizedSlots)
    .sort((a, b) => a.localeCompare(b))
    .map((key) => [key, normalizedSlots[key] ?? null, normalizedOutputs[key] ?? null]);
  return JSON.stringify({ rows, columns, entries });
};
