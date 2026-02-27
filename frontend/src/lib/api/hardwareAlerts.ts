import { browser } from '$app/environment';
import { readable, type Readable } from 'svelte/store';

export type HardwareAlertSeverity = 'error' | 'warning';

export type HardwareAlertAction = {
  label: string;
  href: string;
};

export type HardwareAlert = {
  id: string;
  title: string;
  description: string;
  severity: HardwareAlertSeverity;
  action?: HardwareAlertAction;
};

const DEFAULT_POLL_INTERVAL_MS = 15_000;

export function createHardwareAlertsStore(pollIntervalMs = DEFAULT_POLL_INTERVAL_MS): Readable<HardwareAlert[]> {
  return readable<HardwareAlert[]>([], (set) => {
    if (!browser) {
      return () => {};
    }

    let closed = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    const poll = async () => {
      if (closed) return;
      try {
        // Hardware firmware alert signals are not available in the current OpenAPI surface.
        set([]);
      } catch (error) {
        console.warn('Hardware alert polling failed', error);
      } finally {
        if (!closed) {
          timer = setTimeout(poll, pollIntervalMs);
        }
      }
    };

    poll();

    return () => {
      closed = true;
      if (timer) clearTimeout(timer);
    };
  });
}

function titleCase(value: string): string {
  return value
    .split(/[\s_-]+/)
    .filter(Boolean)
    .map((chunk) => chunk.charAt(0).toUpperCase() + chunk.slice(1).toLowerCase())
    .join(' ');
}
