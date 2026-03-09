import type { DaedalusRegistryType, TypeRegistryLookup } from './types';
import { normalizedString } from './parsing';

const typeRegistryCache = new WeakMap<DaedalusRegistryType[], TypeRegistryLookup | undefined>();

export const buildTypeRegistryLookup = (types: DaedalusRegistryType[] | null | undefined): TypeRegistryLookup | undefined => {
  if (!Array.isArray(types) || types.length === 0) return undefined;
  if (typeRegistryCache.has(types)) {
    return typeRegistryCache.get(types);
  }
  const out: TypeRegistryLookup = new Map();
  for (const entry of types) {
    const key = normalizedString(entry?.rust);
    if (!key || entry?.ty == null) continue;
    out.set(key, entry.ty);
  }
  const resolved = out.size > 0 ? out : undefined;
  typeRegistryCache.set(types, resolved);
  return resolved;
};
