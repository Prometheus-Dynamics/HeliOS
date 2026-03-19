<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';

  type CropRect = { x: number; y: number; width: number; height: number };
  type DragHandle = 'move' | 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw';

  type ImageCropperProps = {
    src: string;
    crop: CropRect;
    rotateDegrees?: number;
    naturalWidth?: number;
    naturalHeight?: number;
    minCropSize?: number;
  };

  const props: ImageCropperProps = $props();

  const dispatch = createEventDispatcher<{ cropChange: CropRect; rotateChange: number }>();

  const cloneCrop = (rect: CropRect): CropRect => ({ x: rect?.x ?? 0, y: rect?.y ?? 0, width: rect?.width ?? 0, height: rect?.height ?? 0 });

  let activeCrop = $derived.by(() => cloneCrop(props.crop));
  let rotationState = $derived.by(() => {
    void props.src;
    return props.rotateDegrees ?? 0;
  });
  let rotationVisual = $derived.by(() => {
    void props.src;
    return props.rotateDegrees ?? 0;
  });

  const imageSrc = $derived.by(() => props.src);
  const propNaturalWidth = $derived.by(() => props.naturalWidth ?? 0);
  const propNaturalHeight = $derived.by(() => props.naturalHeight ?? 0);
  const propMinCropSize = $derived.by(() => props.minCropSize ?? 32);

  let imageElement = $state<HTMLImageElement | null>(null);
  let stageElement = $state<HTMLDivElement | null>(null);
  let resizeObserver: ResizeObserver | null = null;
  let lastObservedElement: Element | null = null;
  let stageWidth = $state(0);
  let stageHeight = $state(0);
  let dragState: {
    handle: DragHandle;
    startPointer: { x: number; y: number };
    startCrop: CropRect;
    rotation: number;
  } | null = null;

  const handleLabels: Record<Exclude<DragHandle, 'move'>, string> = {
    n: 'Resize vertically from top',
    s: 'Resize vertically from bottom',
    e: 'Resize horizontally from right',
    w: 'Resize horizontally from left',
    ne: 'Resize from top right corner',
    nw: 'Resize from top left corner',
    se: 'Resize from bottom right corner',
    sw: 'Resize from bottom left corner'
  };

  function updateDisplaySize() {
    if (!stageElement) return;
    const rect = stageElement.getBoundingClientRect();
    stageWidth = rect.width;
    stageHeight = rect.height;
  }

  function observeImageElement() {
    if (!resizeObserver || !stageElement) return;
    if (lastObservedElement === stageElement) {
      return;
    }
    resizeObserver.disconnect();
    resizeObserver.observe(stageElement);
    lastObservedElement = stageElement;
    updateDisplaySize();
  }

  onMount(() => {
    resizeObserver = new ResizeObserver(() => updateDisplaySize());
    observeImageElement();
    window.addEventListener('resize', updateDisplaySize);
    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener('resize', updateDisplaySize);
    };
  });

  $effect(() => {
    if (resizeObserver && stageElement) {
      observeImageElement();
    }
  });

  const safeMinSize = $derived.by(() => Math.max(8, propMinCropSize));
  const safeNaturalWidth = $derived.by(() => {
    if (propNaturalWidth > 0) return propNaturalWidth;
    if (activeCrop?.width && activeCrop.width > 0) return activeCrop.width;
    return stageWidth || 1;
  });
  const safeNaturalHeight = $derived.by(() => {
    if (propNaturalHeight > 0) return propNaturalHeight;
    if (activeCrop?.height && activeCrop.height > 0) return activeCrop.height;
    return stageHeight || 1;
  });
  function getRotatedBounds(width: number, height: number, degrees: number) {
    if (width <= 0 || height <= 0) {
      return { width: width || 0, height: height || 0 };
    }
    const radians = (degrees * Math.PI) / 180;
    const sin = Math.sin(radians);
    const cos = Math.cos(radians);
    return {
      width: Math.abs(width * cos) + Math.abs(height * sin),
      height: Math.abs(width * sin) + Math.abs(height * cos)
    };
  }

  const rotatedBounds = $derived.by(() => {
    const width = safeNaturalWidth || 1;
    const height = safeNaturalHeight || 1;
    return getRotatedBounds(width, height, rotationVisual);
  });

  const stageSafeWidth = $derived.by(() => (stageWidth > 0 ? stageWidth : safeNaturalWidth));
  const stageSafeHeight = $derived.by(() => (stageHeight > 0 ? stageHeight : safeNaturalHeight));
  const fitScale = $derived.by(() => {
    if (rotatedBounds.width <= 0 || rotatedBounds.height <= 0) return 1;
    const wScale = stageSafeWidth / rotatedBounds.width;
    const hScale = stageSafeHeight / rotatedBounds.height;
    const scale = Math.min(wScale, hScale);
    if (!Number.isFinite(scale) || scale <= 0) return 1;
    return scale;
  });
  const renderWidth = $derived.by(() => safeNaturalWidth * fitScale);
  const renderHeight = $derived.by(() => safeNaturalHeight * fitScale);
  const renderWidthPx = $derived.by(() => Math.max(renderWidth, 0));
  const renderHeightPx = $derived.by(() => Math.max(renderHeight, 0));
  const scaleX = $derived.by(() => (safeNaturalWidth > 0 ? renderWidth / safeNaturalWidth : 0));
  const scaleY = $derived.by(() => (safeNaturalHeight > 0 ? renderHeight / safeNaturalHeight : 0));
  const cropLeft = $derived.by(() => (activeCrop ? activeCrop.x * scaleX : 0));
  const cropTop = $derived.by(() => (activeCrop ? activeCrop.y * scaleY : 0));
  const cropWidthPx = $derived.by(() => (activeCrop ? activeCrop.width * scaleX : 0));
  const cropHeightPx = $derived.by(() => (activeCrop ? activeCrop.height * scaleY : 0));
  const overlayReady = $derived.by(() => scaleX > 0 && scaleY > 0 && cropWidthPx > 0 && cropHeightPx > 0);

  $effect(() => {
    if (activeCrop && safeNaturalWidth > 0 && safeNaturalHeight > 0) {
      const sanitized = sanitizeCrop(activeCrop);
      if (!areRectsEqual(sanitized, activeCrop)) {
        activeCrop = sanitized;
        dispatch('cropChange', sanitized);
      }
    }
  });

  function areRectsEqual(a: CropRect, b: CropRect) {
    return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
  }

  function sanitizeCrop(rect: CropRect): CropRect {
    if (!rect) {
      return {
        x: 0,
        y: 0,
        width: safeNaturalWidth,
        height: safeNaturalHeight
      } satisfies CropRect;
    }
    const minWidth = Math.min(safeMinSize, safeNaturalWidth);
    const minHeight = Math.min(safeMinSize, safeNaturalHeight);
    const maxX = Math.max(0, safeNaturalWidth - minWidth);
    const maxY = Math.max(0, safeNaturalHeight - minHeight);
    const x = clamp(rect.x, 0, maxX);
    const y = clamp(rect.y, 0, maxY);
    const maxWidthForX = safeNaturalWidth - x;
    const maxHeightForY = safeNaturalHeight - y;
    const width = clamp(rect.width, minWidth, maxWidthForX);
    const height = clamp(rect.height, minHeight, maxHeightForY);
    return {
      x: Math.round(x),
      y: Math.round(y),
      width: Math.round(width),
      height: Math.round(height)
    } satisfies CropRect;
  }

  function clamp(value: number, min: number, max: number) {
    if (!Number.isFinite(value)) return min;
    if (min > max) return max;
    return Math.min(Math.max(value, min), max);
  }

  function transformDelta(deltaX: number, deltaY: number, rotation: number) {
    if (!rotation) {
      return { x: deltaX, y: deltaY };
    }
    const radians = (-rotation * Math.PI) / 180;
    const cos = Math.cos(radians);
    const sin = Math.sin(radians);
    return {
      x: deltaX * cos - deltaY * sin,
      y: deltaX * sin + deltaY * cos
    };
  }

  function handlePointerMove(event: PointerEvent) {
    if (!dragState || !activeCrop) return;
    event.preventDefault();
    if (!scaleX || !scaleY) return;
    const deltaScreenX = event.clientX - dragState.startPointer.x;
    const deltaScreenY = event.clientY - dragState.startPointer.y;
    const { x: deltaStageX, y: deltaStageY } = transformDelta(deltaScreenX, deltaScreenY, dragState.rotation);
    const deltaX = deltaStageX / scaleX;
    const deltaY = deltaStageY / scaleY;
    const next: CropRect = { ...dragState.startCrop };
    switch (dragState.handle) {
      case 'move':
        next.x = dragState.startCrop.x + deltaX;
        next.y = dragState.startCrop.y + deltaY;
        break;
      case 'e':
        next.width = dragState.startCrop.width + deltaX;
        break;
      case 'w': {
        const newX = dragState.startCrop.x + deltaX;
        next.width = dragState.startCrop.width + (dragState.startCrop.x - newX);
        next.x = newX;
        break;
      }
      case 's':
        next.height = dragState.startCrop.height + deltaY;
        break;
      case 'n': {
        const newY = dragState.startCrop.y + deltaY;
        next.height = dragState.startCrop.height + (dragState.startCrop.y - newY);
        next.y = newY;
        break;
      }
      case 'ne': {
        next.height = dragState.startCrop.height + deltaY;
        next.width = dragState.startCrop.width + deltaX;
        break;
      }
      case 'nw': {
        const newX = dragState.startCrop.x + deltaX;
        const newY = dragState.startCrop.y + deltaY;
        next.width = dragState.startCrop.width + (dragState.startCrop.x - newX);
        next.height = dragState.startCrop.height + (dragState.startCrop.y - newY);
        next.x = newX;
        next.y = newY;
        break;
      }
      case 'se':
        next.width = dragState.startCrop.width + deltaX;
        next.height = dragState.startCrop.height + deltaY;
        break;
      case 'sw': {
        const newX = dragState.startCrop.x + deltaX;
        next.width = dragState.startCrop.width + (dragState.startCrop.x - newX);
        next.height = dragState.startCrop.height + deltaY;
        next.x = newX;
        break;
      }
    }
    applyCrop(next);
  }

  function stopDrag() {
    dragState = null;
    window.removeEventListener('pointermove', handlePointerMove);
    window.removeEventListener('pointerup', stopDrag);
  }

  function startDrag(handle: DragHandle, event: PointerEvent) {
    if (!activeCrop) return;
    event.preventDefault();
    event.stopPropagation();
    dragState = {
      handle,
      startPointer: { x: event.clientX, y: event.clientY },
      startCrop: { ...activeCrop },
      rotation: rotationState
    };
    window.addEventListener('pointermove', handlePointerMove);
    window.addEventListener('pointerup', stopDrag);
  }

  function applyCrop(next: CropRect) {
    const sanitized = sanitizeCrop(next);
    activeCrop = sanitized;
    dispatch('cropChange', sanitized);
  }

  function rotate(delta: number) {
    rotationVisual += delta;
    rotationState = normalizeRotation(rotationState + delta);
    dispatch('rotateChange', rotationState);
  }

  function normalizeRotation(value: number) {
    if (!Number.isFinite(value)) return 0;
    const normalized = value % 360;
    return normalized === 0 ? 0 : normalized;
  }

  function resetCrop() {
    applyCrop({ x: 0, y: 0, width: safeNaturalWidth, height: safeNaturalHeight });
  }

  function formatRotation(value: number) {
    const normalized = ((value % 360) + 360) % 360;
    return Math.round(normalized);
  }
</script>

<div class="flex w-full min-w-0 flex-col gap-3">
  <div
    class="relative flex min-h-[18rem] w-full min-w-0 flex-1 items-center justify-center overflow-hidden rounded border border-surface-800/60 bg-surface-900/40"
    aria-label="Image cropper"
    style="max-height: 70vh;"
    bind:this={stageElement}
  >
    {#if imageSrc}
      <div class="relative max-w-full" style={`width: ${renderWidthPx}px; height: ${renderHeightPx}px;`}>
        <div
          class="absolute inset-0"
          style={`transform: rotate(${rotationVisual}deg); transform-origin: center; transition: transform 160ms ease-out;`}
        >
          <img
            class="block h-full w-full rounded object-contain"
            src={imageSrc}
            alt="Editable asset"
            bind:this={imageElement}
            onload={updateDisplaySize}
            draggable={false}
          />
        </div>
        {#if overlayReady}
          <div class="pointer-events-none absolute inset-0" aria-hidden="true">
            <div class="absolute left-0 top-0 w-full" style={`height: ${cropTop}px`}>
              <div class="h-full w-full bg-surface-950/60"></div>
            </div>
            <div
              class="absolute left-0"
              style={`top: ${cropTop + cropHeightPx}px; height: ${Math.max(0, renderHeightPx - (cropTop + cropHeightPx))}px; width: ${renderWidthPx}px`}
            >
              <div class="h-full w-full bg-surface-950/60"></div>
            </div>
            <div class="absolute left-0" style={`top: ${cropTop}px; height: ${cropHeightPx}px; width: ${cropLeft}px`}>
              <div class="h-full w-full bg-surface-950/60"></div>
            </div>
            <div
              class="absolute"
              style={`top: ${cropTop}px; left: ${cropLeft + cropWidthPx}px; height: ${cropHeightPx}px; width: ${Math.max(0, renderWidthPx - (cropLeft + cropWidthPx))}px`}
            >
              <div class="h-full w-full bg-surface-950/60"></div>
            </div>
          </div>
          <div
            class="pointer-events-auto absolute border border-primary-400/80 bg-black/10"
            style={`left: ${cropLeft}px; top: ${cropTop}px; width: ${cropWidthPx}px; height: ${cropHeightPx}px;`}
          >
            <div class="pointer-events-none absolute inset-0 grid grid-cols-3 grid-rows-3">
              {#each Array.from({ length: 9 }, (_, index) => index) as index (index)}
                <div class="border border-white/10"></div>
              {/each}
            </div>
            <button
              type="button"
              class="absolute inset-0 cursor-move"
              aria-label="Move crop selection"
              onpointerdown={(event) => startDrag('move', event)}
            ></button>
            <button
              type="button"
              class="pointer-events-auto absolute left-1/2 top-0 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full border border-white/80 bg-surface-900/90"
              aria-label={handleLabels.n}
              onpointerdown={(event) => startDrag('n', event)}
            ></button>
            <button
              type="button"
              class="pointer-events-auto absolute left-1/2 bottom-0 h-3 w-3 -translate-x-1/2 translate-y-1/2 rounded-full border border-white/80 bg-surface-900/90"
              aria-label={handleLabels.s}
              onpointerdown={(event) => startDrag('s', event)}
            ></button>
            <button
              type="button"
              class="pointer-events-auto absolute top-1/2 right-0 h-3 w-3 translate-x-1/2 -translate-y-1/2 rounded-full border border-white/80 bg-surface-900/90"
              aria-label={handleLabels.e}
              onpointerdown={(event) => startDrag('e', event)}
            ></button>
            <button
              type="button"
              class="pointer-events-auto absolute top-1/2 left-0 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full border border-white/80 bg-surface-900/90"
              aria-label={handleLabels.w}
              onpointerdown={(event) => startDrag('w', event)}
            ></button>
            <button
              type="button"
              class="pointer-events-auto absolute left-0 top-0 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full border border-white/80 bg-primary-400"
              aria-label={handleLabels.nw}
              onpointerdown={(event) => startDrag('nw', event)}
            ></button>
            <button
              type="button"
              class="pointer-events-auto absolute right-0 top-0 h-3 w-3 translate-x-1/2 -translate-y-1/2 rounded-full border border-white/80 bg-primary-400"
              aria-label={handleLabels.ne}
              onpointerdown={(event) => startDrag('ne', event)}
            ></button>
            <button
              type="button"
              class="pointer-events-auto absolute right-0 bottom-0 h-3 w-3 translate-x-1/2 translate-y-1/2 rounded-full border border-white/80 bg-primary-400"
              aria-label={handleLabels.se}
              onpointerdown={(event) => startDrag('se', event)}
            ></button>
            <button
              type="button"
              class="pointer-events-auto absolute left-0 bottom-0 h-3 w-3 -translate-x-1/2 translate-y-1/2 rounded-full border border-white/80 bg-primary-400"
              aria-label={handleLabels.sw}
              onpointerdown={(event) => startDrag('sw', event)}
            ></button>
          </div>
        {/if}
      </div>
    {:else}
      <div class="flex h-64 items-center justify-center text-xs text-surface-500">No preview available</div>
    {/if}
    {#if imageSrc}
      <div class="pointer-events-auto absolute right-3 top-3 flex gap-2">
        <button
          type="button"
          class="rounded-full border border-surface-700/70 bg-surface-950/80 p-2 text-xs text-surface-100 transition hover:bg-surface-800/80"
          aria-label="Rotate counter clockwise"
          onclick={() => rotate(-90)}
        >
          ↺
        </button>
        <button
          type="button"
          class="rounded-full border border-surface-700/70 bg-surface-950/80 p-2 text-xs text-surface-100 transition hover:bg-surface-800/80"
          aria-label="Rotate clockwise"
          onclick={() => rotate(90)}
        >
          ↻
        </button>
        <button
          type="button"
          class="rounded-full border border-surface-700/70 bg-surface-950/80 px-3 py-2 text-xs uppercase tracking-[0.2em] text-surface-200 transition hover:bg-surface-800/80"
          onclick={resetCrop}
        >
          Reset
        </button>
      </div>
    {/if}
  </div>
  <div class="flex flex-wrap items-center justify-between gap-2 text-xs text-surface-500">
    <p>
      Crop: {Math.round(activeCrop?.width ?? 0)} × {Math.round(activeCrop?.height ?? 0)} @ ({Math.round(activeCrop?.x ?? 0)},{Math.round(activeCrop?.y ?? 0)})
    </p>
    <p>Rotation: {formatRotation(rotationState)}°</p>
  </div>
</div>

<style>
  .grid div {
    border-color: rgba(255, 255, 255, 0.15);
  }
</style>
