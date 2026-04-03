import { describe, expect, test } from 'bun:test';

import { normalizeArucoBitGrid } from './localizationViewerTagBits';

describe('normalizeArucoBitGrid', () => {
  test('keeps square grids that already include their border', () => {
    const grid = normalizeArucoBitGrid({
      width: 8,
      border: 1,
      rows: [
        '00000000',
        '01111110',
        '01000010',
        '01011010',
        '01011010',
        '01000010',
        '01111110',
        '00000000'
      ]
    });

    expect(grid).not.toBeNull();
    expect(grid?.width).toBe(8);
    expect(grid?.rows[0]).toBe('00000000');
    expect(grid?.rows[7]).toBe('00000000');
  });

  test('pads data-only rows with the declared border', () => {
    const grid = normalizeArucoBitGrid({
      width: 6,
      border: 1,
      rows: ['111111', '100001', '101101', '101101', '100001', '111111']
    });

    expect(grid).not.toBeNull();
    expect(grid?.width).toBe(8);
    expect(grid?.rows[0]).toBe('00000000');
    expect(grid?.rows[1]).toBe('01111110');
    expect(grid?.rows[6]).toBe('01111110');
    expect(grid?.rows[7]).toBe('00000000');
  });

  test('ignores stale width metadata when rows already form a square bordered grid', () => {
    const grid = normalizeArucoBitGrid({
      width: 6,
      border: 1,
      rows: [
        '00000000',
        '01111110',
        '01000010',
        '01011010',
        '01011010',
        '01000010',
        '01111110',
        '00000000'
      ]
    });

    expect(grid).not.toBeNull();
    expect(grid?.width).toBe(8);
  });

  test('rejects non-square malformed grids instead of drawing fake bits', () => {
    const grid = normalizeArucoBitGrid({
      width: 6,
      border: 1,
      rows: ['111111', '100001', '101101']
    });

    expect(grid).toBeNull();
  });
});
