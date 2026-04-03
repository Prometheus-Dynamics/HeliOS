import { readable } from 'svelte/store';
import { browser } from '$app/environment';
import { apiFetch } from '$lib/api/core/http';
import type { Nt4Settings, Nt4TopicsResponse, TeamNumberPayload } from '$lib/api/client';

export type RobotConnectionState = {
  status: 'connected' | 'disconnected' | 'unconfigured' | 'subscriptions_disabled';
  host: string | null;
  port: number | null;
  error: string | null;
  updatedAt: number;
};

function rioServerHostFromTeam(team: number | null | undefined): string | null {
  if (typeof team !== 'number' || !Number.isFinite(team) || !Number.isInteger(team) || team <= 0) return null;
  const a = Math.floor(team / 100);
  const b = team % 100;
  if (a < 0 || a > 255 || b < 0 || b > 255) return null;
  return `10.${a}.${b}.2`;
}

function normalizeHost(raw: string | null | undefined): string | null {
  if (typeof raw !== 'string') return null;
  const trimmed = raw.trim();
  return trimmed.length ? trimmed : null;
}

async function fetchRobotConnection(): Promise<RobotConnectionState> {
  const now = Date.now();

  const [team, nt4] = await Promise.all([
    apiFetch<TeamNumberPayload>('/device/team').catch(() => null),
    apiFetch<Nt4Settings>('/device/nt4').catch(() => null)
  ]);

  const teamNumber = typeof team?.team_number === 'number' ? team.team_number : null;
  const hostOverride = normalizeHost(nt4?.server_host ?? null);
  const host = hostOverride ?? rioServerHostFromTeam(teamNumber);
  const port = typeof nt4?.server_port === 'number' && Number.isFinite(nt4.server_port) ? nt4.server_port : 5810;
  const subscriptionsEnabled = nt4?.subscriptions_enabled ?? true;

  if (!host) {
    return { status: 'unconfigured', host: null, port: null, error: null, updatedAt: now };
  }

  if (!subscriptionsEnabled) {
    return { status: 'subscriptions_disabled', host, port, error: null, updatedAt: now };
  }

  try {
    // Lightweight reachability check: ask backend to connect to NT4 and list just a handful of topics.
    await apiFetch<Nt4TopicsResponse>(
      '/nt4/topics',
      {
        method: 'POST',
        body: { host, port, prefix: '/', scan_ms: 250, limit: 32 }
      },
      { timeoutMs: 1200, recordConnection: false }
    );
    return { status: 'connected', host, port, error: null, updatedAt: now };
  } catch (err) {
    const message = err instanceof Error ? err.message : 'NT4 probe failed';
    return { status: 'disconnected', host, port, error: message, updatedAt: now };
  }
}

export const robotConnection = readable<RobotConnectionState>(
  { status: 'unconfigured', host: null, port: null, error: null, updatedAt: Date.now() },
  (set) => {
    if (!browser) return () => {};

    let cancelled = false;
    let timer: number | null = null;

    const tick = async () => {
      try {
        const next = await fetchRobotConnection();
        if (!cancelled) set(next);
      } catch {
        // ignore
      } finally {
        if (!cancelled) timer = window.setTimeout(tick, 2500);
      }
    };

    void tick();

    return () => {
      cancelled = true;
      if (timer != null) window.clearTimeout(timer);
    };
  }
);
