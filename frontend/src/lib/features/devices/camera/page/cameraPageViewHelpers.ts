export type StreamViewerBounds = { width: number; height: number };

export const setupStreamViewerResize = (
  host: HTMLDivElement | null,
  onResize: (bounds: StreamViewerBounds) => void
): (() => void) | undefined => {
  if (!host) return undefined;
  const ro = new ResizeObserver((entries) => {
    const rect = entries[0]?.contentRect;
    if (!rect) return;
    onResize({ width: Math.floor(rect.width), height: Math.floor(rect.height) });
  });
  ro.observe(host);
  return () => ro.disconnect();
};
