import type { PipelineStatus } from '$lib/types/pipeline';

export function formatStatus(status: PipelineStatus): string {
  switch (status) {
    case 'live':
      return 'Live';
    case 'degraded':
      return 'Degraded';
    default:
      return 'OK';
  }
}

export function statusBadgeClass(status: PipelineStatus): string {
  if (status === 'live') return 'bg-emerald-500/20 text-emerald-200 border border-emerald-500/40';
  if (status === 'degraded') return 'bg-amber-500/20 text-amber-200 border border-amber-500/40';
  return 'bg-surface-800/80 text-surface-200 border border-surface-700/80';
}
