import type { LogEntry } from './types';

export function syncLogEntryIds(entries: LogEntry[], set: Set<string>): void {
  set.clear();
  for (const entry of entries) {
    set.add(entry.id);
  }
}

export function trimLogEntries(entries: LogEntry[], max: number): LogEntry[] {
  return entries.length > max ? entries.slice(-max) : entries;
}

export function mergeLogEntries(
  existing: LogEntry[],
  pending: LogEntry[],
  idSet: Set<string>,
  max: number
): { entries: LogEntry[]; changed: boolean } {
  if (!pending.length) return { entries: existing, changed: false };
  let merged = existing.slice();
  let changed = false;
  for (const entry of pending) {
    if (!idSet.has(entry.id)) {
      idSet.add(entry.id);
      merged.push(entry);
      changed = true;
    }
  }
  if (!changed) return { entries: existing, changed: false };
  if (merged.length > max) {
    merged = merged.slice(-max);
    syncLogEntryIds(merged, idSet);
  }
  return { entries: merged, changed: true };
}
