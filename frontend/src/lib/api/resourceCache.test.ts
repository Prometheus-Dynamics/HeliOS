import { describe, expect, test } from 'bun:test';

import { cacheResourceData, cacheResourceNotModified, isResourceCacheResult } from './resourceCache';

describe('resourceCache', () => {
  test('wraps fresh data with cache metadata', () => {
    const result = cacheResourceData(['a', 'b'], { etag: 'W/"rev-7"', revision: 7 });

    expect(result).toEqual({
      __helios_resource_cache__: true,
      status: 'data',
      data: ['a', 'b'],
      etag: 'W/"rev-7"',
      revision: 7
    });
    expect(isResourceCacheResult(result)).toBe(true);
  });

  test('wraps not-modified responses with null defaults', () => {
    const result = cacheResourceNotModified<string[]>();

    expect(result).toEqual({
      __helios_resource_cache__: true,
      status: 'not_modified',
      etag: null,
      revision: null
    });
    expect(isResourceCacheResult(result)).toBe(true);
    expect(isResourceCacheResult({ status: 'not_modified' })).toBe(false);
  });
});
