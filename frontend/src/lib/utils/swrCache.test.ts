import { describe, expect, test } from 'bun:test';

import { invalidateSWR, invalidateSWRPrefix, primeSWR, readSWR, revalidateSWR } from './swrCache';

describe('swrCache', () => {
  test('revalidateSWR stores metadata returned by a cache-aware fetcher', async () => {
    const key = `cache:test:${Date.now()}:meta`;
    invalidateSWR(key);

    const value = await revalidateSWR(
      key,
      async () => ({
        __heliosSwrWrite: true as const,
        data: ['one', 'two'],
        meta: { etag: 'W/"rev-11"', revision: 11 }
      }),
      { staleMs: 0, maxAgeMs: 60_000, force: true }
    );

    expect(value).toEqual(['one', 'two']);

    const snapshot = readSWR<string[]>(key, { staleMs: 60_000, maxAgeMs: 60_000 });
    expect(snapshot?.data).toEqual(['one', 'two']);
    expect(snapshot?.etag).toBe('W/"rev-11"');
    expect(snapshot?.revision).toBe(11);

    invalidateSWR(key);
  });

  test('invalidateSWRPrefix removes only matching cached entries', () => {
    const prefix = `cache:test:${Date.now()}:prefix`;
    const keepKey = `${prefix}:keep`;
    const dropKeyA = `${prefix}:drop:a`;
    const dropKeyB = `${prefix}:drop:b`;

    primeSWR(dropKeyA, ['a'], { etag: 'etag-a', revision: 1 });
    primeSWR(dropKeyB, ['b'], { etag: 'etag-b', revision: 2 });
    primeSWR(keepKey, ['keep'], { etag: 'etag-keep', revision: 3 });

    invalidateSWRPrefix(`${prefix}:drop`);

    expect(readSWR(dropKeyA)).toBeNull();
    expect(readSWR(dropKeyB)).toBeNull();
    expect(readSWR<string[]>(keepKey)?.data).toEqual(['keep']);

    invalidateSWR(keepKey);
  });
});
