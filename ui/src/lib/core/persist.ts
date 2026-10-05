// Small, failure-tolerant localStorage helpers. Preferences and layouts live
// here for now; on a device they move to the per-user config on the node.

export function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(`helios:${key}`);
    return raw ? { ...fallback, ...JSON.parse(raw) } : fallback;
  } catch {
    return fallback;
  }
}

export function loadRaw<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(`helios:${key}`);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

export function save(key: string, value: unknown) {
  try {
    localStorage.setItem(`helios:${key}`, JSON.stringify(value));
  } catch {
    // storage unavailable (private window, quota): preferences just don't persist
  }
}
