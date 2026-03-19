import type { ArucoBitGrid } from './localizationViewerTypes';

function sanitizeRows(rows: string[] | undefined | null): string[] {
  if (!Array.isArray(rows)) return [];
  return rows
    .map((row) => (typeof row === 'string' ? row.trim().replace(/[^01]/g, '') : ''))
    .filter((row) => row.length > 0);
}

function buildBorderedGrid(rows: string[], border: number): string[] {
  if (border <= 0 || rows.length === 0) return rows;
  const dataWidth = rows[0]?.length ?? 0;
  if (!dataWidth) return rows;
  const totalWidth = dataWidth + border * 2;
  const borderRow = '0'.repeat(totalWidth);
  const out: string[] = [];
  for (let i = 0; i < border; i += 1) {
    out.push(borderRow);
  }
  for (const row of rows) {
    out.push(`${'0'.repeat(border)}${row}${'0'.repeat(border)}`);
  }
  for (let i = 0; i < border; i += 1) {
    out.push(borderRow);
  }
  return out;
}

function hasSolidBlackBorder(rows: string[], border: number): boolean {
  if (border <= 0 || rows.length === 0) return true;
  const width = rows.length;
  if (!rows.every((row) => row.length === width)) return false;
  for (let y = 0; y < width; y += 1) {
    const row = rows[y] ?? '';
    for (let x = 0; x < width; x += 1) {
      const onBorder = x < border || y < border || x >= width - border || y >= width - border;
      if (onBorder && row.charCodeAt(x) !== 48) {
        return false;
      }
    }
  }
  return true;
}

export function normalizeArucoBitGrid(bits: ArucoBitGrid | null | undefined): ArucoBitGrid | null {
  if (!bits) return null;
  const border = Number.isFinite(bits.border) ? Math.max(0, Math.trunc(bits.border)) : 0;
  const rows = sanitizeRows(bits.rows);
  if (rows.length === 0) return null;

  const widths = Array.from(new Set(rows.map((row) => row.length)));
  if (widths.length !== 1) return null;
  const rowWidth = widths[0] ?? 0;
  if (rowWidth <= 0) return null;

  if (rows.length !== rowWidth) {
    return null;
  }

  const normalizedRows = border > 0 && !hasSolidBlackBorder(rows, border) ? buildBorderedGrid(rows, border) : rows;
  const normalizedWidth = normalizedRows.length;
  if (normalizedWidth === 0 || !normalizedRows.every((row) => row.length === normalizedWidth)) {
    return null;
  }

  return {
    width: normalizedWidth,
    border,
    rows: normalizedRows
  };
}
