export type DebounceHandle = ReturnType<typeof setTimeout> | null;

export function scheduleDebounce(
  handle: DebounceHandle,
  fn: () => void,
  delayMs: number
): DebounceHandle {
  if (handle) {
    clearTimeout(handle);
  }
  return setTimeout(fn, delayMs);
}

export function cancelDebounce(handle: DebounceHandle): null {
  if (handle) {
    clearTimeout(handle);
  }
  return null;
}
