export type PipelineTuningPointerState = {
  get pipelineTuningPanelOpen(): boolean;
  get pipelineTuningDragState(): { startX: number; startY: number; originX: number; originY: number } | null;
  set pipelineTuningDragState(value: { startX: number; startY: number; originX: number; originY: number } | null);
  get pipelineTuningResizeState(): { startX: number; startY: number; originW: number; originH: number } | null;
  set pipelineTuningResizeState(value: { startX: number; startY: number; originW: number; originH: number } | null);
  get pipelineTuningPosition(): { x: number; y: number };
  set pipelineTuningPosition(value: { x: number; y: number });
  get pipelineTuningSize(): { width: number; height: number };
  set pipelineTuningSize(value: { width: number; height: number });
};

export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

export function createPipelineTuningPointerHandlers(state: PipelineTuningPointerState) {
  function onPipelineTuningPointerMove(event: PointerEvent): void {
    if (typeof window === 'undefined') return;
    if (state.pipelineTuningDragState) {
      const dx = event.clientX - state.pipelineTuningDragState.startX;
      const dy = event.clientY - state.pipelineTuningDragState.startY;
      const maxX = Math.max(0, window.innerWidth - state.pipelineTuningSize.width - 12);
      const maxY = Math.max(0, window.innerHeight - state.pipelineTuningSize.height - 12);
      state.pipelineTuningPosition = {
        x: clamp(state.pipelineTuningDragState.originX + dx, 8, maxX),
        y: clamp(state.pipelineTuningDragState.originY + dy, 8, maxY)
      };
      return;
    }
    if (state.pipelineTuningResizeState) {
      const dx = event.clientX - state.pipelineTuningResizeState.startX;
      const dy = event.clientY - state.pipelineTuningResizeState.startY;
      const minW = 460;
      const minH = 420;
      const maxW = Math.max(minW, window.innerWidth - state.pipelineTuningPosition.x - 12);
      const maxH = Math.max(minH, window.innerHeight - state.pipelineTuningPosition.y - 12);
      state.pipelineTuningSize = {
        width: clamp(state.pipelineTuningResizeState.originW + dx, minW, maxW),
        height: clamp(state.pipelineTuningResizeState.originH + dy, minH, maxH)
      };
    }
  }

  function onPipelineTuningPointerUp(): void {
    state.pipelineTuningDragState = null;
    state.pipelineTuningResizeState = null;
    stopPipelineTuningPointerTracking();
  }

  function stopPipelineTuningPointerTracking(): void {
    // Window listeners are handled in the component via <svelte:window />.
  }

  function startPipelineTuningPointerTracking(): void {
    // Window listeners are handled in the component via <svelte:window />.
  }

  function shouldIgnoreDragTarget(target: HTMLElement | null): boolean {
    if (!target) return false;
    if (target.closest('button, input, select, textarea, a')) return true;
    return false;
  }

  function startPipelineTuningDrag(event: PointerEvent | MouseEvent): void {
    if (state.pipelineTuningResizeState) return;
    const target = event.target as HTMLElement | null;
    if (shouldIgnoreDragTarget(target)) return;
    state.pipelineTuningDragState = {
      startX: event.clientX,
      startY: event.clientY,
      originX: state.pipelineTuningPosition.x,
      originY: state.pipelineTuningPosition.y
    };
  }

  function startPipelineTuningResize(event: PointerEvent): void {
    if (state.pipelineTuningDragState) return;
    event.preventDefault();
    event.stopPropagation();
    state.pipelineTuningResizeState = {
      startX: event.clientX,
      startY: event.clientY,
      originW: state.pipelineTuningSize.width,
      originH: state.pipelineTuningSize.height
    };
  }

  return {
    onPipelineTuningPointerMove,
    onPipelineTuningPointerUp,
    startPipelineTuningPointerTracking,
    stopPipelineTuningPointerTracking,
    startPipelineTuningDrag,
    startPipelineTuningResize
  };
}
