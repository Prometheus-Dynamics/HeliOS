// Settings sync. The device announces every change to its settings as one `settings` event
// (`{domain, key}`, see helios-api's Store); the simulated device does the same across tabs. Each
// store registers for its domain and refetches what changed, so every open tab follows.

import type { SettingsChange } from "#lib/api/types.js";

type Handler = (key: string) => void;

const handlers = new Map<string, Set<Handler>>();

/** Call `handler` with the key whenever a setting of `domain` changes. Returns the unsubscribe. */
export function onSettingsChange(domain: string, handler: Handler): () => void {
  const set = handlers.get(domain) ?? new Set<Handler>();
  set.add(handler);
  handlers.set(domain, set);
  return () => set.delete(handler);
}

/** A change of a setting, from the device (or the simulated one): run its domain's handlers. */
export function dispatchSettings(change: SettingsChange) {
  if (typeof change?.domain !== "string") return;
  for (const handler of handlers.get(change.domain) ?? []) handler(String(change.key ?? ""));
}
