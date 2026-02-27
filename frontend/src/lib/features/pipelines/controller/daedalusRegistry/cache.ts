import type { DaedalusRegistryType, TypeRegistryLookup } from './types';
import { normalizedString } from './parsing';

export const buildTypeRegistryLookup = (types: DaedalusRegistryType[] | null | undefined): TypeRegistryLookup | undefined => {
  if (!Array.isArray(types) || types.length === 0) return undefined;
  const out: TypeRegistryLookup = new Map();
  for (const entry of types) {
    const key = normalizedString(entry?.rust);
    if (!key || entry?.ty == null) continue;
    out.set(key, entry.ty);
  }
  return out.size > 0 ? out : undefined;
};
