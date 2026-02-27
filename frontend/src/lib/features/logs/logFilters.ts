import type { LogFilter } from './types';

export const LOG_LEVEL_ALIASES: Record<string, Exclude<LogFilter, 'all'>> = {
  info: 'info',
  notice: 'info',
  warn: 'warn',
  warning: 'warn',
  error: 'error',
  err: 'error',
  fatal: 'error',
  critical: 'error',
  debug: 'debug',
  trace: 'debug'
};

export function normalizeLevelToken(token: string): Exclude<LogFilter, 'all'> | null {
  const normalized = token.trim().toLowerCase();
  if (!normalized) return null;
  return LOG_LEVEL_ALIASES[normalized] ?? null;
}
