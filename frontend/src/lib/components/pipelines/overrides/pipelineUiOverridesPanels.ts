import { normalizeGridOutputKeys } from '$lib/features/devices/camera/page/cameraPipelineState';
import type { PipelineUiLayoutDefinition } from '$lib/features/pipelines/pipelineUiTypes';

export function computeFloatingPanelStyle(options: {
  position: { x: number; y: number } | null;
  selectedItemAnchor: { x: number; y: number } | null;
  fallback: string;
  width: number;
  height: number;
  offsetX: number;
  offsetY: number;
}): string {
  const { position, selectedItemAnchor, fallback, width, height, offsetX, offsetY } = options;
  if (position) {
    return `left: ${position.x}px; top: ${position.y}px;`;
  }
  if (!selectedItemAnchor) return fallback;

  const baseX = selectedItemAnchor.x + offsetX;
  const baseY = selectedItemAnchor.y + offsetY;
  if (typeof window === 'undefined') {
    return `left: ${baseX}px; top: ${baseY}px;`;
  }

  const x = Math.max(16, Math.min(baseX, window.innerWidth - width - 16));
  const y = Math.max(16, Math.min(baseY, window.innerHeight - height - 16));
  return `left: ${x}px; top: ${y}px;`;
}

export function startFloatingPanelDrag(
  event: PointerEvent,
  selector: string,
  begin: (offset: { x: number; y: number }, rect: DOMRect) => void,
  move: (position: { x: number; y: number }) => void,
  end: () => void
): void {
  const target = event.target as HTMLElement;
  if (target.closest('button, input, select, textarea')) return;
  const panelEl = (event.currentTarget as HTMLElement).closest(selector) as HTMLElement | null;
  if (!panelEl) return;

  event.preventDefault();
  const rect = panelEl.getBoundingClientRect();
  const offset = { x: event.clientX - rect.left, y: event.clientY - rect.top };
  begin(offset, rect);

  const handleMove = (moveEvent: PointerEvent) => {
    move({
      x: Math.max(8, moveEvent.clientX - offset.x),
      y: Math.max(8, moveEvent.clientY - offset.y)
    });
  };
  const handleUp = () => {
    end();
    window.removeEventListener('pointermove', handleMove);
    window.removeEventListener('pointerup', handleUp);
  };

  window.addEventListener('pointermove', handleMove);
  window.addEventListener('pointerup', handleUp);
}

export function clampGridSize(value: number): number {
  const numeric = Math.trunc(Number(value));
  if (!Number.isFinite(numeric)) return 1;
  return Math.min(6, Math.max(1, numeric));
}

export function normalizeLayoutEditorState(
  layout: PipelineUiLayoutDefinition | null | undefined
): { rows: number; columns: number; outputKeys: Record<string, string | null> } {
  const rows = clampGridSize(layout?.rows ?? 1);
  const columns = clampGridSize(layout?.columns ?? 1);
  const outputKeys = normalizeGridOutputKeys(rows, columns, layout?.outputKeys ?? {});
  return { rows, columns, outputKeys };
}
