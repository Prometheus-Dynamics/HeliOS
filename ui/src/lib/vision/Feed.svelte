<script lang="ts">
  import StatusChain from "#lib/components/status/StatusChain.svelte";
  // A live camera frame fitted into its box (letterboxed, never cropped) with
  // detection overlays, an ROI you can draw and drag, a crosshair, a grid and
  // a luminance histogram. Overlays draw in sensor pixels via one SVG.
  import type { Camera, Detection } from "#lib/api/model.js";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import type { Overlays } from "./overlays";
  import { field } from "#lib/stores/field.svelte.js";
  import { featureSummaryText, toFeedSpace, trackFill } from "./features";
  import { depthRgba } from "./dense";
  import { openPreview, previewSocketUrl } from "./preview-socket";
  import { scaleIntrinsics, tagAxes, type AxisSegment } from "./pose";


  let {
    camera,
    overlays,
    roiTool = false,
    onroi,
    compact = false,
  }: {
    camera: Camera;
    overlays: Overlays;
    roiTool?: boolean;
    onroi?: (roi: Camera["settings"]["roi"]) => void;
    compact?: boolean;
  } = $props();

  const CORNER_COLORS = ["#ff4d4d", "#4ade80", "#60a5fa", "#facc15"];
  const index = $derived(cluster.feedIndex(camera));
  const live = $derived(camera.feed.live);
  const src = $derived(live ?? `${camera.feed.base}/${String(index).padStart(4, "0")}.jpg`);
  // The live preview's own size (scaled by the device, 640x400 by default) once it loads.
  let natural = $state<{ w: number; h: number } | null>(null);
  let previewFailed = $state(false);
  // The live preview comes over its WebSocket, each frame with its overlays, drawn together on a
  // canvas; the MJPEG <img> is the fallback when the socket cannot be used.
  let socketFailed = $state(false);
  const socketMode = $derived(Boolean(live) && !socketFailed && typeof WebSocket !== "undefined" && Boolean(cluster.live));
  // A picture (or its element) is on screen. Without one the status chain takes the box instead.
  const previewing = $derived(socketMode || (Boolean(live) && !previewFailed) || Boolean(camera.feed.base));
  let screen = $state<HTMLCanvasElement>();
  $effect(() => {
    void live;
    natural = null;
    previewFailed = false;
    socketFailed = false;
  });
  $effect(() => {
    if (!socketMode || !live || !screen || !cluster.live) return;
    const target = screen;
    const store = cluster.live;
    const stopOverlay = store.beginFrameOverlay(camera.resourceId);
    const close = openPreview(
      previewSocketUrl(live, location.href),
      (frame) => {
        if (!natural || natural.w !== frame.width || natural.h !== frame.height) natural = { w: frame.width, h: frame.height };
        if (target.width !== frame.bitmap.width || target.height !== frame.bitmap.height) {
          target.width = frame.bitmap.width;
          target.height = frame.bitmap.height;
        }
        target.getContext("2d")?.drawImage(frame.bitmap, 0, 0);
        frame.bitmap.close();
        // The outputs of this very frame, so the SVG redraws with the image.
        store.applyFrameOverlay(camera.resourceId, frame.timestampNs);
        if (overlays.histogram && frame.sequence % 8 === 0) sample(true);
      },
      () => (socketFailed = true),
    );
    return () => {
      close();
      stopOverlay();
    };
  });
  // Feed frames are a scaled replay; overlays use the feed's pixel space and
  // the ROI is stored in sensor pixels.
  const fw = $derived(live ? (natural?.w ?? 640) : (cluster.feed?.width ?? 640));
  const fh = $derived(live ? (natural?.h ?? 400) : (cluster.feed?.height ?? 400));
  // Detection corners scaled into the feed's pixel space (the SVG's viewBox): live corners are in
  // the pipeline's frame (e.g. 1280x800) while the preview is smaller (640x400 by default).
  const markers = $derived(
    cluster.detectionsFor(camera).map((d) =>
      d.frame ? { ...d, corners: d.corners.map(([x, y]) => [(x * fw) / d.frame!.w, (y * fh) / d.frame!.h] as [number, number]) } : d,
    ),
  );
  // Feature tracks (a `tracks` output) scaled into the feed's pixels the way the corners are.
  const features = $derived(cluster.featureTracksFor(camera));
  const featureView = $derived(features && overlays.features ? toFeedSpace(features, fw, fh) : null);
  // The dense mapper's depth of this camera, painted over the image (the same field of view: the
  // sweep covers the whole frame, so scaling it to the feed aligns it with the image).
  const dense = $derived(overlays.depth ? cluster.denseForCamera(camera) : null);
  let depthCanvas = $state<HTMLCanvasElement>();
  $effect(() => {
    const depth = dense?.depth;
    const target = depthCanvas;
    if (!depth || !target) return;
    target.width = depth.width;
    target.height = depth.height;
    target.getContext("2d")?.putImageData(new ImageData(depthRgba(depth), depth.width, depth.height), 0, 0);
  });
  // Each tag's axes from its pose (camera-from-tag) and the camera's intrinsics, in the feed's pixels.
  const poses = $derived(overlays.poseAxes ? cluster.poseViewForCamera(camera) : null);
  const intrinsics = $derived(overlays.poseAxes ? cluster.intrinsicsFor(camera) : null);
  const axes = $derived.by(() => {
    if (!poses?.tags || !intrinsics) return [] as { id: number; segments: AxisSegment[] }[];
    const k = scaleIntrinsics(intrinsics, fw, fh);
    const size = field.layout.tagSize;
    return poses.tags.tags.flatMap((t) => {
      const segments = tagAxes(t.pose, size, k);
      return segments ? [{ id: t.id, segments }] : [];
    });
  });
  const sx = $derived(camera.settings.width / fw);
  const sy = $derived(camera.settings.height / fh);
  const tint = $derived(colorVar(identity.get(camera.resourceId).color));

  let host = $state<HTMLDivElement>();
  let box = $state({ w: 0, h: 0 });
  $effect(() => {
    if (!host) return;
    const ro = new ResizeObserver(([e]) => (box = { w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(host);
    return () => ro.disconnect();
  });
  const fit = $derived.by(() => {
    const scale = Math.min(box.w / fw, box.h / fh) || 0;
    return { w: fw * scale, h: fh * scale, scale };
  });
  const k = $derived(fit.scale ? 1 / fit.scale : 1); // feed px per screen px

  // --- ROI drawing (in feed pixels while dragging) -------------------------
  let drag = $state<{ x0: number; y0: number; x1: number; y1: number; mode: "new" | "move"; ox?: number; oy?: number } | null>(null);
  const roi = $derived(camera.settings.roi ? { x: camera.settings.roi.x / sx, y: camera.settings.roi.y / sy, w: camera.settings.roi.w / sx, h: camera.settings.roi.h / sy } : null);
  const shown = $derived.by(() => {
    if (!drag) return roi;
    if (drag.mode === "move" && roi) return { ...roi, x: roi.x + drag.x1 - drag.x0, y: roi.y + drag.y1 - drag.y0 };
    return { x: Math.min(drag.x0, drag.x1), y: Math.min(drag.y0, drag.y1), w: Math.abs(drag.x1 - drag.x0), h: Math.abs(drag.y1 - drag.y0) };
  });

  function toFeed(event: PointerEvent) {
    const r = (event.currentTarget as SVGElement).getBoundingClientRect();
    return { x: Math.max(0, Math.min(fw, ((event.clientX - r.left) / r.width) * fw)), y: Math.max(0, Math.min(fh, ((event.clientY - r.top) / r.height) * fh)) };
  }
  function down(event: PointerEvent) {
    if (!roiTool) return;
    const p = toFeed(event);
    const inside = roi && p.x >= roi.x && p.x <= roi.x + roi.w && p.y >= roi.y && p.y <= roi.y + roi.h;
    drag = { x0: p.x, y0: p.y, x1: p.x, y1: p.y, mode: inside ? "move" : "new" };
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
  }
  function move(event: PointerEvent) {
    if (!drag) return;
    const p = toFeed(event);
    drag.x1 = p.x;
    drag.y1 = p.y;
  }
  function up() {
    if (!drag) return;
    const s = shown;
    drag = null;
    if (!s || s.w < 8 || s.h < 8) return;
    const even = (v: number) => Math.round(v / 2) * 2;
    onroi?.({ x: even(s.x * sx), y: even(s.y * sy), w: even(Math.min(s.w, fw - s.x) * sx), h: even(Math.min(s.h, fh - s.y) * sy) });
  }

  // --- Histogram: sampled from the decoded frame every few frames ----------
  let img = $state<HTMLImageElement>();
  let hist = $state<number[]>([]);
  let canvas: HTMLCanvasElement | null = null;
  function loaded() {
    if (live && img?.naturalWidth) natural = { w: img.naturalWidth, h: img.naturalHeight };
    sample();
  }
  // An MJPEG stream fires `load` once, so the live preview is sampled on a timer.
  $effect(() => {
    if (!live || socketMode || !overlays.histogram) return;
    const timer = setInterval(() => sample(true), 500);
    return () => clearInterval(timer);
  });
  function sample(force = false) {
    const source: CanvasImageSource | undefined = socketMode ? screen : img;
    if (!overlays.histogram || !source || (!socketMode && (!img?.complete || !img.naturalWidth)) || (!force && index % 4)) return;
    canvas ??= document.createElement("canvas");
    canvas.width = 128;
    canvas.height = 80;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) return;
    ctx.drawImage(source, 0, 0, 128, 80);
    let data: Uint8ClampedArray;
    try {
      data = ctx.getImageData(0, 0, 128, 80).data;
    } catch {
      return; // A preview from another origin (a UI served elsewhere) cannot be read back.
    }
    const bins = new Array(64).fill(0);
    for (let i = 0; i < data.length; i += 4) bins[(data[i] * 0.3 + data[i + 1] * 0.59 + data[i + 2] * 0.11) >> 2]++;
    const max = Math.max(...bins);
    hist = bins.map((b) => b / max);
  }
  const histPath = $derived(hist.length ? `M0,40 ${hist.map((v, i) => `L${(i / 63) * 128},${40 - v * 38}`).join(" ")} L128,40Z` : "");
  const clipped = $derived(hist.length ? hist[63] > 0.25 : false);

  function pick(m: Detection) {
    selection.select({ kind: "tag", id: String(m.id), parent: camera.resourceId });
  }
</script>

<div class="feed" bind:this={host} style:--tint={tint}>
  <div class="frame" style:width="{fit.w}px" style:height="{fit.h}px">
    {#if socketMode}
      <canvas bind:this={screen} class="screen" aria-label="{camera.name} camera"></canvas>
    {:else if (live && !previewFailed) || camera.feed.base}
      <img bind:this={img} {src} alt="{camera.name} camera" draggable="false" onload={loaded} onerror={() => live && (previewFailed = true)} />
    {:else}
      <div class="no-preview">
        <b>No preview</b>
        {#if camera.status?.blocking}
          <StatusChain status={camera.status} variant="why" />
        {:else}
          <span>{live ? "The camera's preview did not load. Its camera service may still be starting." : "This camera has no camera service to preview."}</span>
        {/if}
      </div>
    {/if}
    {#if overlays.depth && dense?.depth}
      <canvas bind:this={depthCanvas} class="depth" aria-label="Depth preview, frame {dense.depth.frame}"></canvas>
    {/if}
    <svg
      viewBox="0 0 {fw} {fh}"
      class:tool={roiTool}
      role="presentation"
      onpointerdown={down}
      onpointermove={move}
      onpointerup={up}
    >
      {#if overlays.grid}
        {#each [1, 2] as g (g)}
          <line x1={(fw * g) / 3} y1="0" x2={(fw * g) / 3} y2={fh} class="grid" stroke-width={k} />
          <line x1="0" y1={(fh * g) / 3} x2={fw} y2={(fh * g) / 3} class="grid" stroke-width={k} />
        {/each}
      {/if}
      {#if overlays.crosshair}
        <line x1={fw / 2 - 14 * k} y1={fh / 2} x2={fw / 2 + 14 * k} y2={fh / 2} class="cross" stroke-width={1.5 * k} />
        <line x1={fw / 2} y1={fh / 2 - 14 * k} x2={fw / 2} y2={fh / 2 + 14 * k} class="cross" stroke-width={1.5 * k} />
      {/if}
      {#if shown}
        <path d="M0,0H{fw}V{fh}H0Z M{shown.x},{shown.y}v{shown.h}h{shown.w}v{-shown.h}Z" class="roi-dim" fill-rule="evenodd" />
        <rect x={shown.x} y={shown.y} width={shown.w} height={shown.h} class="roi" stroke-width={1.5 * k} stroke-dasharray="{6 * k} {4 * k}" />
        {#if !compact}
          <text x={shown.x + 4 * k} y={shown.y - 5 * k} class="roi-label" style:font-size="{11 * k}px">ROI {Math.round(shown.w * sx)}×{Math.round(shown.h * sy)}</text>
        {/if}
      {/if}
      {#if featureView}
        {#each featureView.trails as [id, points] (id)}
          <polyline points={points.map((p) => `${p.x},${p.y}`).join(" ")} class="trail" stroke-width={1.2 * k} />
        {/each}
        {#each featureView.tracks as t (t.id)}
          <circle cx={t.x} cy={t.y} r={(t.status === "new" ? 3.2 : 2.6) * k} class="track" stroke-width={1 * k} style:fill={trackFill(t)} />
        {/each}
      {/if}
      <!-- Keyed by position too: a scene can show the same tag id twice (a test field, a duplicate print). -->
      {#each axes as tag, i (`${tag.id}:${i}`)}
        {#each tag.segments as s (s.axis)}
          <line x1={s.x0} y1={s.y0} x2={s.x1} y2={s.y1} class="axis {s.axis}" stroke-width={3.4 * k} />
        {/each}
      {/each}
      {#each markers as m (m.id + ":" + m.corners[0][0])}
        {@const sel = selection.is("tag", String(m.id))}
        {#if overlays.outlines}
          <polygon
            points={m.corners.map((c) => c.join(",")).join(" ")}
            class="outline"
            class:sel
            stroke-width={(sel ? 3 : 2) * k}
            role="button"
            tabindex="-1"
            onpointerdown={(e) => { if (!roiTool) { e.stopPropagation(); pick(m); } }}
          />
        {/if}
        {#if overlays.corners}
          {#each m.corners as c, i (i)}
            <circle cx={c[0]} cy={c[1]} r={3 * k} fill={CORNER_COLORS[i]} />
          {/each}
        {/if}
        {#if overlays.ids}
          {@const cx = m.corners.reduce((s, c) => s + c[0], 0) / 4}
          {@const cy = m.corners.reduce((s, c) => s + c[1], 0) / 4}
          <text x={cx} y={cy} class="id" style:font-size="{(compact ? 11 : 14) * k}px" stroke-width={3 * k}>{m.id}</text>
        {/if}
      {/each}
    </svg>
    {#if overlays.hud}
      <div class="hud tl">
        <span class="dot"></span>
        <b>{camera.name}</b>
        {#if !compact}<span>{camera.settings.width}×{camera.settings.height}</span>{/if}
      </div>
      <div class="hud bl mono">
        {camera.stats.fps.toFixed(0)} fps · {camera.stats.latencyMs.toFixed(1)} ms{#if !compact} · {markers.length} tags · #{index}{/if}
      </div>
    {/if}
    {#if features && featureView}
      <span class="hud fe mono">{featureSummaryText(features.tracks.summary, compact)}</span>
    {/if}
    {#if overlays.histogram && histPath && !compact}
      <svg class="hist" viewBox="0 0 128 40" preserveAspectRatio="none" aria-label="Brightness histogram">
        <path d={histPath} />
        {#if clipped}<rect x="124" y="0" width="4" height="40" class="clip" />{/if}
      </svg>
    {/if}
    {#if camera.foreign}<span class="hud tr">via {camera.foreign}</span>{/if}
  </div>
  {#if previewing && camera.status}
    <div class="status-strip"><StatusChain status={camera.status} variant="strip" /></div>
  {/if}
</div>

<style>
  .feed {
    position: relative;
    flex: 1;
    min-height: 0;
    min-width: 0;
    display: grid;
    place-items: center;
    background: #050608;
    overflow: hidden;
  }
  .frame {
    position: relative;
  }
  /* The status chain over the picture: one line, expanding upwards. Clear of the stats line and the histogram. */
  .status-strip {
    position: absolute;
    left: 6px;
    bottom: 30px;
    z-index: 1;
    max-width: min(560px, calc(100% - 170px));
  }
  img,
  .screen,
  .depth,
  svg:not(.hist) {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  img {
    user-select: none;
  }
  .screen {
    display: block;
    user-select: none;
  }
  .no-preview {
    position: absolute;
    inset: 0;
    display: grid;
    place-content: center;
    gap: 4px;
    text-align: center;
    color: rgba(255, 255, 255, 0.6);
    font-size: 12px;
    background: repeating-linear-gradient(135deg, #0b0d11 0 12px, #0e1116 12px 24px);
  }
  .no-preview b {
    color: rgba(255, 255, 255, 0.85);
    font-size: 13px;
  }
  svg.tool {
    cursor: crosshair;
  }
  .grid {
    stroke: rgba(255, 255, 255, 0.22);
  }
  .cross {
    stroke: var(--tint);
  }
  .roi-dim {
    fill: rgba(0, 0, 0, 0.45);
    pointer-events: none;
  }
  .roi {
    fill: none;
    stroke: var(--warn);
    pointer-events: none;
  }
  .roi-label {
    fill: var(--warn);
    font-family: var(--font-code);
    font-weight: 600;
    pointer-events: none;
  }
  .outline {
    fill: rgba(34, 211, 238, 0.1);
    stroke: #22d3ee;
    stroke-linejoin: round;
    cursor: pointer;
  }
  .outline.sel {
    stroke: var(--accent);
    fill: color-mix(in oklab, var(--accent) 22%, transparent);
  }
  .id {
    fill: #fff;
    font-family: var(--font-ui);
    font-weight: 700;
    text-anchor: middle;
    dominant-baseline: central;
    paint-order: stroke;
    stroke: rgba(0, 0, 0, 0.8);
    pointer-events: none;
  }
  circle {
    pointer-events: none;
  }
  .trail {
    fill: none;
    stroke: rgba(34, 211, 238, 0.6);
    stroke-linejoin: round;
    stroke-linecap: round;
    pointer-events: none;
  }
  .track {
    stroke: rgba(0, 0, 0, 0.7);
  }
  .depth {
    opacity: 0.6;
    pointer-events: none;
  }
  .axis {
    stroke-linecap: round;
    pointer-events: none;
  }
  .axis.x {
    stroke: #ff5f5f;
  }
  .axis.y {
    stroke: #5fff8a;
  }
  .axis.z {
    stroke: #5fb4ff;
  }
  .hud {
    position: absolute;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 7px;
    font-size: 11px;
    color: #e8eaf0;
    background: rgba(0, 0, 0, 0.55);
    border-radius: var(--r-1);
    pointer-events: none;
    backdrop-filter: blur(4px);
  }
  .hud b {
    font-weight: 650;
  }
  .hud .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--tint);
  }
  .tl {
    top: 6px;
    left: 6px;
  }
  .tr {
    top: 6px;
    right: 6px;
    color: var(--warn);
  }
  .bl {
    bottom: 6px;
    left: 6px;
  }
  .fe {
    top: 30px;
    left: 6px;
    max-width: calc(100% - 12px);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #a5f3fc;
  }
  .mono {
    font-family: var(--font-code);
    font-size: 10.5px;
  }
  .hist {
    position: absolute;
    right: 6px;
    bottom: 6px;
    width: 150px;
    height: 48px;
    background: rgba(0, 0, 0, 0.55);
    border-radius: var(--r-1);
    pointer-events: none;
  }
  .hist path {
    fill: rgba(255, 255, 255, 0.7);
  }
  .hist .clip {
    fill: var(--err);
  }
</style>
