export type VirtualViewportSize = {
  height: number;
  width: number;
};

export type VirtualViewportCallbacks = {
  onScroll?: (scrollTop: number) => void;
  onResize?: (size: VirtualViewportSize) => void;
};

export type VirtualWindow = {
  startIndex: number;
  endIndex: number;
  offset: number;
  totalHeight: number;
};

type VirtualWindowInput = {
  itemCount: number;
  rowHeight: number;
  overscan: number;
  scrollTop: number;
  viewportHeight: number;
};

export function getVirtualWindow({
  itemCount,
  rowHeight,
  overscan,
  scrollTop,
  viewportHeight
}: VirtualWindowInput): VirtualWindow {
  if (itemCount <= 0 || rowHeight <= 0) {
    return { startIndex: 0, endIndex: 0, offset: 0, totalHeight: 0 };
  }
  const safeViewport = Math.max(1, viewportHeight || 1);
  const startIndex = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const endIndex = Math.min(itemCount, Math.ceil((scrollTop + safeViewport) / rowHeight) + overscan);
  const offset = startIndex * rowHeight;
  const totalHeight = itemCount * rowHeight;
  return { startIndex, endIndex, offset, totalHeight };
}

export function virtualViewport(node: HTMLElement, callbacks: VirtualViewportCallbacks = {}) {
  let current = callbacks;
  let observer: ResizeObserver | null = null;

  function emitScroll(): void {
    current.onScroll?.(node.scrollTop);
  }

  function emitResize(): void {
    current.onResize?.({ height: node.clientHeight, width: node.clientWidth });
  }

  node.addEventListener('scroll', emitScroll, { passive: true });
  emitResize();
  emitScroll();

  if (typeof ResizeObserver !== 'undefined') {
    observer = new ResizeObserver(() => emitResize());
    observer.observe(node);
  }

  return {
    update(next: VirtualViewportCallbacks = {}) {
      current = next;
      emitResize();
      emitScroll();
    },
    destroy() {
      node.removeEventListener('scroll', emitScroll);
      if (observer) {
        observer.disconnect();
        observer = null;
      }
    }
  };
}
