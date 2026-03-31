import { writable } from 'svelte/store';
import type { ValidationIssue } from '$lib/ts-bindings/http/client';
import { readJson, writeJson } from '$lib/utils/storage';

export type NotificationKind = 'error' | 'warning' | 'info';

export type NotificationItem = {
  id: string;
  title: string;
  description?: string;
  kind: NotificationKind;
  createdAt: number;
  occurredAt?: number | null;
  source?: string;
  operation?: string | null;
  code?: string | null;
  requestId?: string | null;
  traceId?: string | null;
  retryable?: boolean | null;
  remediation?: string | null;
  reportedBy?: string | null;
  validationIssues?: ValidationIssue[] | null;
};

const MAX_NOTIFICATIONS = 50;
const STORAGE_KEY = 'helios.notifications.v1';

function buildId(): string {
  return `n-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

function loadStoredNotifications(): NotificationItem[] {
  const parsed = readJson<NotificationItem[]>(STORAGE_KEY, []);
  if (!Array.isArray(parsed)) return [];
  return parsed.filter((item) => typeof item.id === 'string');
}

export const notifications = writable<NotificationItem[]>(loadStoredNotifications());

function persistNotifications(list: NotificationItem[]): void {
  writeJson(STORAGE_KEY, list);
}

export function pushNotification(
  input: Omit<NotificationItem, 'id' | 'createdAt'>,
): NotificationItem {
  const item: NotificationItem = {
    id: buildId(),
    createdAt: Date.now(),
    ...input
  };
  notifications.update((list) => {
    const next = [item, ...list];
    if (next.length > MAX_NOTIFICATIONS) {
      next.length = MAX_NOTIFICATIONS;
    }
    persistNotifications(next);
    return next;
  });
  return item;
}

export function dismissNotification(id: string): void {
  notifications.update((list) => {
    const next = list.filter((item) => item.id !== id);
    persistNotifications(next);
    return next;
  });
}

export function clearNotifications(): void {
  notifications.set([]);
  persistNotifications([]);
}

export function mergeNotifications(incoming: NotificationItem[]): void {
  notifications.update((list) => {
    if (!incoming.length) return list;
    const seen = new Set<string>();
    const combined = [...incoming, ...list].filter((item) => {
      if (seen.has(item.id)) return false;
      seen.add(item.id);
      return true;
    });
    if (combined.length > MAX_NOTIFICATIONS) {
      combined.length = MAX_NOTIFICATIONS;
    }
    persistNotifications(combined);
    return combined;
  });
}
