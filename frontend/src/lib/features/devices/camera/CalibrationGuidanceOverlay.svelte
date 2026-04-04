<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { PipelinesApi } from '$lib/api/pipelinesApi';
  import { loadOwnedStreamCapabilities } from '$lib/api/streamResources';
  import { StreamsApi } from '$lib/api/streamsApi';
  import {
    averageAreaRatio,
    averageSkewRatio,
    bitCount,
    clamp,
    colorForId,
    computeGuidanceStatus,
    cornerCoverageCount,
    cornerCoverageMask,
    detectionExtentRatio,
    edgeCoverageCount,
    frameCoverageRatio,
    normalizeDetections,
    quadArea,
    scoreSnapshot,
    statusColor,
    type Corner2D,
    type Detection,
    type GuidanceStatus
  } from './calibrationGuidanceAnalysis';
  import {
    clearGuidedState,
    defaultCaptureStats,
    getGuidedState,
    type CaptureStatsState
  } from './calibrationGuidanceStore';

  type ContentRect = { x: number; y: number; width: number; height: number };

  const props = $props<{
    enabled: boolean;
    streamUuid: string;
    sourceResolution: { width: number; height: number } | null;
    resetToken: number;
    captureToken: number;
    accumulateLive: boolean;
    fitMode?: 'contain' | 'cover';
    drawOverlay?: boolean;
  }>();

  let host = $state<HTMLDivElement | null>(null);
  let canvas = $state<HTMLCanvasElement | null>(null);
  let ctx2d = $state<CanvasRenderingContext2D | null>(null);
  let ro: ResizeObserver | null = null;
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let metricsTimer: ReturnType<typeof setInterval> | null = null;
  let outputPort = $state<string | null>(null);
  let activePipelineId = $state<string | null>(null);
  let activePipelineOutput = $state<string | null>(null);
  let graphName = $state<string>('daedalus_aruco');
  let graphMs = $state<number | null>(null);
  let detectMs = $state<number | null>(null);
  let outputMs = $state<number | null>(null);
  let lastDetections = $state<Detection[]>([]);
  let lastNonEmptyDetections = $state<Detection[]>([]);
  let lastNonEmptyAt = $state(0);
  let coverage = $state<Float32Array>(new Float32Array(0));

  let grid = $state({ cols: 12, rows: 8 });
  let pendingCapture = $state(false);
  let lastGridKey = $state<string>('');
  let lastCaptureAt = $state(0);
  let loadedStream: string | null = null;
  let lastResetToken: number | null = null;
  let lastCaptureToken: number | null = null;
  let captureStats = $state<CaptureStatsState>(defaultCaptureStats());

  const CLOSE_EXTENT_RATIO = 0.12;
  const FAR_EXTENT_RATIO = 0.05;
  const TARGET_CLOSE = 1;
  const TARGET_FAR = 1;
  const TARGET_SKEW = 1;
  const TARGET_CORNERS = 2;
  let calibrationModePipelineUuid = $state('');
  let rawPipelineUuid = $state('');
  let streamCapabilitiesLoaded = false;
  let streamCapabilitiesPromise: Promise<void> | null = null;
  const graphNameCache = new SvelteMap<string, string>();

  function persistGuidedState(streamUuid: string): void {
    const saved = getGuidedState(streamUuid);
    saved.captureStats = { ...captureStats };
    saved.grid = { ...grid };
    saved.coverage = new Float32Array(coverage);
    saved.lastCaptureAt = lastCaptureAt;
    saved.lastGridKey = lastGridKey;
  }

  function resetCoverage(nextGrid?: { cols: number; rows: number }): void {
    const target = nextGrid ?? grid;
    coverage = new Float32Array(target.cols * target.rows);
  }

  $effect(() => {
    const token = props.resetToken;
    if (token === lastResetToken) return;
    lastResetToken = token;
    resetCoverage(untrack(() => grid));
    captureStats = defaultCaptureStats();
    lastCaptureAt = 0;
    if (props.streamUuid) {
      clearGuidedState(props.streamUuid);
    }
    loadedStream = null;
    outputPort = null;
    activePipelineId = null;
    activePipelineOutput = null;
    graphName = 'daedalus_aruco';
    graphMs = null;
    detectMs = null;
    outputMs = null;
  });

  $effect(() => {
    const streamUuid = props.streamUuid;
    if (!streamUuid) return;
    if (streamUuid === loadedStream) return;
    loadedStream = streamUuid;
    outputPort = null;
    activePipelineId = null;
    activePipelineOutput = null;
    graphName = 'daedalus_aruco';
    graphMs = null;
    detectMs = null;
    outputMs = null;
    const saved = getGuidedState(streamUuid);
    captureStats = { ...saved.captureStats };
    grid = { ...saved.grid };
    coverage = new Float32Array(saved.coverage);
    lastCaptureAt = saved.lastCaptureAt;
    lastGridKey = saved.lastGridKey;
  });

  $effect(() => {
    const token = props.captureToken;
    if (token === lastCaptureToken) return;
    lastCaptureToken = token;
    if (!props.accumulateLive) {
      untrack(() => {
        const detsForCapture = getCaptureDetections();
        updateCoverage(detsForCapture);
        updateCaptureStats(detsForCapture, true);
      });
      pendingCapture = false;
    } else {
      pendingCapture = true;
    }
  });

  $effect(() => {
    const res = props.sourceResolution;
    if (!res || !Number.isFinite(res.width) || !Number.isFinite(res.height) || res.width <= 0 || res.height <= 0) return;
    const cellSize = 40;
    const cols = clamp(Math.round(res.width / cellSize), 8, 64);
    const rows = clamp(Math.round(res.height / cellSize), 6, 48);
    const key = `${cols}x${rows}`;
    if (key === lastGridKey) return;
    lastGridKey = key;
    grid = { cols, rows };
    resetCoverage({ cols, rows });
    draw();
  });

  function updateCaptureStats(dets: Detection[], countIfEmpty: boolean): void {
    const frameCoverage = frameCoverageRatio(dets, props.sourceResolution, grid);
    const extentRatio = detectionExtentRatio(dets, props.sourceResolution);
    const corners = cornerCoverageCount(dets, props.sourceResolution);
    const skewRatio = averageSkewRatio(dets);

    const close = extentRatio >= CLOSE_EXTENT_RATIO;
    const far = extentRatio > 0 && extentRatio < FAR_EXTENT_RATIO;
    const skew = skewRatio < 0.8;
    const hitCorners = corners > 0;
    const nextCornerMask = captureStats.cornerMask | cornerCoverageMask(dets, props.sourceResolution);
    const nextCornersUnique = bitCount(nextCornerMask);

    const hasDetections = dets.length > 0;
    const count = countIfEmpty || hasDetections;
    if (!count) return;

    const nextSamples = captureStats.coverageSamples + 1;
    const nextCoverageAvg =
      ((captureStats.coverageAvg * captureStats.coverageSamples) + (frameCoverage * 100)) / nextSamples;

    captureStats = {
      total: captureStats.total + 1,
      close: captureStats.close + (close ? 1 : 0),
      far: captureStats.far + (far ? 1 : 0),
      skew: captureStats.skew + (skew ? 1 : 0),
      corners: captureStats.corners + (hitCorners ? 1 : 0),
      cornerMask: nextCornerMask,
      cornersUnique: nextCornersUnique,
      coverageAvg: nextCoverageAvg,
      coverageSamples: nextSamples
    };
    if (props.streamUuid) persistGuidedState(props.streamUuid);
  }

  function getCaptureDetections(): Detection[] {
    if (lastDetections.length) return lastDetections;
    if (lastNonEmptyDetections.length && Date.now() - lastNonEmptyAt < 1500) {
      return lastNonEmptyDetections;
    }
    return lastDetections;
  }

  function setCanvasSize(): void {
    if (!canvas || !host) return;
    const rect = host.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(rect.width * dpr));
    const h = Math.max(1, Math.round(rect.height * dpr));
    if (canvas.width !== w) canvas.width = w;
    if (canvas.height !== h) canvas.height = h;
    canvas.style.width = `${rect.width}px`;
    canvas.style.height = `${rect.height}px`;
  }

  function contentRect(): ContentRect | null {
    const res = props.sourceResolution;
    if (!canvas || !res || !Number.isFinite(res.width) || !Number.isFinite(res.height) || res.width <= 0 || res.height <= 0) return null;

    const frameW = res.width;
    const frameH = res.height;
    const hostW = canvas.width;
    const hostH = canvas.height;
    if (!Number.isFinite(hostW) || !Number.isFinite(hostH) || hostW <= 0 || hostH <= 0) return null;

    const mode = props.fitMode === 'cover' ? 'cover' : 'contain';
    const scale = mode === 'cover' ? Math.max(hostW / frameW, hostH / frameH) : Math.min(hostW / frameW, hostH / frameH);
    const width = frameW * scale;
    const height = frameH * scale;
    const x = (hostW - width) * 0.5;
    const y = (hostH - height) * 0.5;
    return { x, y, width, height };
  }

  function toCanvasPoint(xPx: number, yPx: number, rect?: ContentRect): { x: number; y: number } | null {
    const res = props.sourceResolution;
    if (!canvas || !res || !Number.isFinite(res.width) || !Number.isFinite(res.height) || res.width <= 0 || res.height <= 0) return null;
    const content = rect ?? contentRect();
    if (!content) return null;
    const nx = xPx / res.width;
    const ny = yPx / res.height;
    return { x: content.x + nx * content.width, y: content.y + ny * content.height };
  }

  function touchCoverageAt(xPx: number, yPx: number): void {
    const res = props.sourceResolution;
    if (!res) return;
    const nx = clamp(xPx / res.width, 0, 0.999999);
    const ny = clamp(yPx / res.height, 0, 0.999999);
    const col = Math.floor(nx * grid.cols);
    const row = Math.floor(ny * grid.rows);
    const idx = row * grid.cols + col;
    if (idx < 0 || idx >= coverage.length) return;
    coverage[idx] = Math.min(coverage[idx] + 1, 255);
  }

  function updateCoverage(dets: Detection[]): void {
    const res = props.sourceResolution;
    if (!res) return;
    if (coverage.length !== grid.cols * grid.rows) resetCoverage();
    for (const det of dets) {
      const corners = det.corners ?? [];
      if (corners.length < 4) continue;
      let cx = 0;
      let cy = 0;
      for (const c of corners) {
        touchCoverageAt(c.x, c.y);
        cx += c.x;
        cy += c.y;
      }
      touchCoverageAt(cx / corners.length, cy / corners.length);
    }
    if (props.streamUuid) persistGuidedState(props.streamUuid);
  }

  function draw(): void {
    if (!ctx2d || !canvas) return;
    ctx2d.clearRect(0, 0, canvas.width, canvas.height);
    if (props.drawOverlay === false) return;
    const content = contentRect();
    if (!content) return;

    // Heatmap: highlight unvisited cells + strengthen visited.
    const cellW = content.width / grid.cols;
    const cellH = content.height / grid.rows;
    let visited = 0;
    for (let r = 0; r < grid.rows; r++) {
      for (let c = 0; c < grid.cols; c++) {
        const idx = r * grid.cols + c;
        const count = coverage[idx] ?? 0;
        if (count > 0) visited += 1;
        // Base "needs data" tint.
        if (count <= 0) {
          ctx2d.fillStyle = 'rgba(255, 64, 64, 0.08)';
          ctx2d.fillRect(content.x + c * cellW, content.y + r * cellH, cellW, cellH);
        } else {
          const alpha = clamp(count / 6, 0, 1) * 0.22;
          ctx2d.fillStyle = `rgba(64, 255, 128, ${alpha})`;
          ctx2d.fillRect(content.x + c * cellW, content.y + r * cellH, cellW, cellH);
        }
      }
    }
    ctx2d.strokeStyle = 'rgba(255,255,255,0.10)';
    ctx2d.lineWidth = Math.max(1, Math.round(content.width / 1200));
    for (let c = 1; c < grid.cols; c++) {
      const x = content.x + c * cellW;
      ctx2d.beginPath();
      ctx2d.moveTo(x, content.y);
      ctx2d.lineTo(x, content.y + content.height);
      ctx2d.stroke();
    }
    for (let r = 1; r < grid.rows; r++) {
      const y = content.y + r * cellH;
      ctx2d.beginPath();
      ctx2d.moveTo(content.x, y);
      ctx2d.lineTo(content.x + content.width, y);
      ctx2d.stroke();
    }

    // Current detections.
    for (const det of lastDetections) {
      const corners = det.corners ?? [];
      if (corners.length < 4) continue;
      const pts = corners.map((c) => toCanvasPoint(c.x, c.y, content)).filter(Boolean) as Array<{ x: number; y: number }>;
      if (pts.length < 4) continue;
      const color = colorForId(Number(det.id) || 0);

      ctx2d.fillStyle = color.replace('hsl', 'hsla').replace(')', ', 0.12)');
      ctx2d.strokeStyle = color.replace('hsl', 'hsla').replace(')', ', 0.95)');
      ctx2d.lineWidth = Math.max(2, Math.round(content.width / 900));

      ctx2d.beginPath();
      ctx2d.moveTo(pts[0].x, pts[0].y);
      for (let i = 1; i < pts.length; i++) ctx2d.lineTo(pts[i].x, pts[i].y);
      ctx2d.closePath();
      ctx2d.fill();
      ctx2d.stroke();

      const dotRadius = Math.max(3, Math.round(content.width / 220));
      ctx2d.lineWidth = Math.max(1, Math.round(dotRadius / 2));
      ctx2d.fillStyle = 'rgba(255,255,255,0.95)';
      ctx2d.strokeStyle = 'rgba(0,0,0,0.85)';
      for (const p of pts) {
        ctx2d.beginPath();
        ctx2d.arc(p.x, p.y, dotRadius, 0, Math.PI * 2);
        ctx2d.fill();
        ctx2d.stroke();
      }

      let cx = 0;
      let cy = 0;
      for (const p of pts) {
        cx += p.x;
        cy += p.y;
      }
      cx /= pts.length;
      cy /= pts.length;
      ctx2d.font = `${Math.max(12, Math.round(content.width / 90))}px ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Arial`;
      ctx2d.textAlign = 'center';
      ctx2d.textBaseline = 'middle';
      ctx2d.fillStyle = 'rgba(0,0,0,0.65)';
      ctx2d.fillText(String(det.id), cx + 1, cy + 1);
      ctx2d.fillStyle = 'rgba(255,255,255,0.95)';
      ctx2d.fillText(String(det.id), cx, cy);
    }

    // HUD.
    const total = grid.cols * grid.rows;
    const coveragePct = total > 0 ? Math.round((visited / total) * 100) : 0;
    const tags = lastDetections.length;
    const line1 = `CALIBRATION • ${graphName} • port ${activePipelineOutput ?? 'n/a'}`;
    const line2 = `${tags} tags • ${coveragePct}% coverage • graph ${formatMs(graphMs)} • detect ${formatMs(detectMs)} • output ${formatMs(outputMs)}`;
    const hudFont = Math.max(11, Math.round(content.width / 130));
    const lineGap = Math.max(2, Math.round(hudFont * 0.34));
    ctx2d.font = `${hudFont}px ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Arial`;
    ctx2d.textAlign = 'left';
    ctx2d.textBaseline = 'top';
    const pad = Math.max(6, Math.round(content.width / 220));
    const metricsLine1 = ctx2d.measureText(line1);
    const metricsLine2 = ctx2d.measureText(line2);
    const hudBoxW = Math.max(metricsLine1.width, metricsLine2.width) + pad * 2;
    const hudBoxH = (hudFont * 2) + lineGap + pad * 2;
    ctx2d.fillStyle = 'rgba(0,0,0,0.55)';
    ctx2d.fillRect(content.x + pad, content.y + pad, hudBoxW, hudBoxH);
    ctx2d.fillStyle = 'rgba(255,255,255,0.95)';
    const textX = content.x + pad * 2;
    const textY = content.y + pad + Math.max(1, Math.round(hudFont * 0.05));
    ctx2d.fillText(line1, textX, textY);
    ctx2d.fillText(line2, textX, textY + hudFont + lineGap);

    const avgArea = averageAreaRatio(lastDetections, props.sourceResolution);
    const frameCoverage = frameCoverageRatio(lastDetections, props.sourceResolution, grid);
    const edges = edgeCoverageCount(lastDetections, props.sourceResolution);
    const score = scoreSnapshot(lastDetections, avgArea, frameCoverage, edges);
    const status = computeGuidanceStatus(lastDetections, props.sourceResolution);

    const hue = Math.round(10 + score * 110);
    const borderColor = `hsla(${hue}, 85%, 55%, 0.75)`;
    const borderWidth = Math.max(2, Math.round(content.width / 260));
    ctx2d.strokeStyle = borderColor;
    ctx2d.lineWidth = borderWidth;
    ctx2d.strokeRect(
      content.x + borderWidth / 2,
      content.y + borderWidth / 2,
      Math.max(1, content.width - borderWidth),
      Math.max(1, content.height - borderWidth)
    );

    const chip = statusColor(status.tone);
    const chipPad = Math.max(6, Math.round(content.width / 210));
    const mainChipFont = Math.max(13, Math.round(content.width / 95));
    const subChipFont = Math.max(11, Math.round(content.width / 120));
    ctx2d.font = `${mainChipFont}px ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Arial`;
    ctx2d.textAlign = 'left';
    ctx2d.textBaseline = 'middle';
    const chipText = status.label;
    const chipMetrics = ctx2d.measureText(chipText);
    const chipW = chipMetrics.width + chipPad * 2.4;
    const chipH = Math.max(20, Math.round(content.height / 28));
    const chipX = Math.max(content.x + chipPad, content.x + content.width - chipW - chipPad);
    const chipY = content.y + chipPad;
    ctx2d.fillStyle = chip.fill;
    ctx2d.strokeStyle = chip.stroke;
    ctx2d.lineWidth = Math.max(1, Math.round(borderWidth / 2));
    ctx2d.beginPath();
    const radius = Math.max(4, Math.round(chipH / 3.2));
    if (typeof ctx2d.roundRect === 'function') {
      ctx2d.roundRect(chipX, chipY, chipW, chipH, radius);
    } else {
      ctx2d.rect(chipX, chipY, chipW, chipH);
    }
    ctx2d.fill();
    ctx2d.stroke();
    ctx2d.fillStyle = chip.text;
    ctx2d.fillText(chipText, chipX + chipPad * 1.1, chipY + chipH / 2);

    const needs = [
      { label: `Closer ${captureStats.close}/${TARGET_CLOSE}`, pending: captureStats.close < TARGET_CLOSE },
      { label: `Farther ${captureStats.far}/${TARGET_FAR}`, pending: captureStats.far < TARGET_FAR },
      { label: `Skewed ${captureStats.skew}/${TARGET_SKEW}`, pending: captureStats.skew < TARGET_SKEW },
      { label: `Corners ${captureStats.cornersUnique}/${TARGET_CORNERS}`, pending: captureStats.cornersUnique < TARGET_CORNERS }
    ];

    const subPadX = Math.max(6, Math.round(content.width / 230));
    const gapY = Math.max(4, Math.round(content.width / 220));
    let nextY = chipY + chipH + gapY;
    const subRadius = Math.max(4, Math.round(chipH / 3.8));

    ctx2d.textBaseline = 'middle';
    ctx2d.font = `${subChipFont}px ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Arial`;
    for (const item of needs) {
      const metrics = ctx2d.measureText(item.label);
      const h = Math.max(18, Math.round(content.height / 34));
      const w = metrics.width + subPadX * 2;
      const x = Math.max(content.x + chipPad, content.x + content.width - w - chipPad);
      const y = nextY;

      const fill = item.pending ? 'rgba(245, 158, 11, 0.78)' : 'rgba(34, 197, 94, 0.7)';
      const stroke = item.pending ? 'rgba(255, 226, 140, 0.82)' : 'rgba(160, 255, 200, 0.8)';
      const text = item.pending ? 'rgba(24, 24, 24, 0.96)' : 'rgba(10,10,10,0.95)';

      ctx2d.fillStyle = fill;
      ctx2d.strokeStyle = stroke;
      ctx2d.lineWidth = Math.max(1, Math.round(borderWidth / 2));
      ctx2d.beginPath();
      if (typeof ctx2d.roundRect === 'function') {
        ctx2d.roundRect(x, y, w, h, subRadius);
      } else {
        ctx2d.rect(x, y, w, h);
      }
      ctx2d.fill();
      ctx2d.stroke();
      ctx2d.fillStyle = text;
      ctx2d.fillText(item.label, x + subPadX, y + h / 2);

      nextY += h + gapY;
    }
  }

  function normalizePipelineId(value: unknown): string | null {
    if (typeof value !== 'string') return null;
    const normalized = value.trim().toLowerCase();
    return normalized.length ? normalized : null;
  }

  async function ensureStreamCapabilities(): Promise<void> {
    if (streamCapabilitiesLoaded) return;
    if (streamCapabilitiesPromise) return streamCapabilitiesPromise;
    streamCapabilitiesPromise = (async () => {
      try {
        const capabilities = await loadOwnedStreamCapabilities();
        const rawId = normalizePipelineId(capabilities?.rawPipelineId);
        if (rawId) rawPipelineUuid = rawId;
        const calibrationId = normalizePipelineId(capabilities?.calibrationModePipelineId);
        if (calibrationId) calibrationModePipelineUuid = calibrationId;
      } catch {
        // Keep previously loaded IDs; avoid local hardcoded fallback IDs.
      } finally {
        streamCapabilitiesLoaded = true;
        streamCapabilitiesPromise = null;
      }
    })();
    return streamCapabilitiesPromise;
  }

  function readErrorStatus(error: unknown): number | null {
    if (!error || typeof error !== 'object') return null;
    const status = Number((error as { status?: unknown }).status);
    return Number.isFinite(status) ? status : null;
  }

  function formatMs(value: number | null): string {
    if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) return 'n/a';
    if (value < 10) return `${value.toFixed(2)}ms`;
    return `${value.toFixed(1)}ms`;
  }

  function readNodeAverageMs(nodesRaw: unknown, candidates: string[]): number | null {
    if (!nodesRaw || typeof nodesRaw !== 'object') return null;
    const nodes = nodesRaw as Record<string, unknown>;
    for (const id of candidates) {
      const entry = nodes[id] as { metrics?: { average_time_ms?: unknown } } | undefined;
      const value = Number(entry?.metrics?.average_time_ms);
      if (Number.isFinite(value) && value >= 0) return value;
    }
    for (const entry of Object.values(nodes)) {
      if (!entry || typeof entry !== 'object') continue;
      const nodeTypeRaw = (entry as { node_type?: unknown }).node_type;
      const nodeType = typeof nodeTypeRaw === 'string' ? nodeTypeRaw : '';
      if (!nodeType) continue;
      if (!candidates.includes(nodeType)) continue;
      const value = Number(((entry as { metrics?: { average_time_ms?: unknown } }).metrics?.average_time_ms));
      if (Number.isFinite(value) && value >= 0) return value;
    }
    return null;
  }

  function pickPipelineMetrics(metricsRaw: unknown, pipelineId: string | null): { nodes?: unknown } | null {
    if (!metricsRaw || typeof metricsRaw !== 'object') return null;
    const metrics = metricsRaw as { pipeline_instances?: unknown; pipeline?: unknown };
    if (pipelineId && metrics.pipeline_instances && typeof metrics.pipeline_instances === 'object') {
      const entry = (metrics.pipeline_instances as Record<string, unknown>)[pipelineId];
      if (entry && typeof entry === 'object') return entry as { nodes?: unknown };
    }
    if (metrics.pipeline && typeof metrics.pipeline === 'object') {
      return metrics.pipeline as { nodes?: unknown };
    }
    return null;
  }

  async function resolveGraphName(pipelineId: string | null): Promise<string> {
    if (!pipelineId) return 'none';
    if (pipelineId === rawPipelineUuid) return 'raw';
    if (pipelineId === calibrationModePipelineUuid) return 'daedalus_aruco';
    const cached = graphNameCache.get(pipelineId);
    if (cached) return cached;
    try {
      const payload = await PipelinesApi.fetchGraph({ id: pipelineId });
      const nameRaw = payload && typeof payload === 'object' ? (payload as { name?: unknown }).name : null;
      const name = typeof nameRaw === 'string' && nameRaw.trim().length ? nameRaw.trim() : pipelineId;
      graphNameCache.set(pipelineId, name);
      return name;
    } catch {
      return pipelineId;
    }
  }

  async function pollMetricsOnce(): Promise<void> {
    if (!props.enabled) return;
    if (!props.streamUuid) return;
    try {
      const [streamPayload, metricsPayload] = await Promise.all([
        StreamsApi.getStream({ id: props.streamUuid }),
        StreamsApi.getMetrics({ id: props.streamUuid })
      ]);
      const manifest = streamPayload && typeof streamPayload === 'object'
        ? (streamPayload as { manifest?: { active_pipeline_id?: unknown; active_pipeline_output?: unknown } }).manifest
        : null;
      const nextPipelineId = normalizePipelineId(manifest?.active_pipeline_id);
      const nextOutput = typeof manifest?.active_pipeline_output === 'string' ? manifest.active_pipeline_output : null;

      if (nextPipelineId !== activePipelineId) {
        activePipelineId = nextPipelineId;
        graphName = await resolveGraphName(nextPipelineId);
      } else if (!graphName) {
        graphName = await resolveGraphName(nextPipelineId);
      }
      activePipelineOutput = nextOutput;

      const selectedPipelineMetrics = pickPipelineMetrics(metricsPayload, nextPipelineId);
      const nodes = selectedPipelineMetrics?.nodes ?? null;
      graphMs = readNodeAverageMs(nodes, ['graph']);
      detectMs = readNodeAverageMs(nodes, ['cv:aruco:detect_detections', 'cv:aruco:decode_quads_hamming']);
      outputMs = readNodeAverageMs(nodes, ['io.host_output']);
      draw();
    } catch {
      // ignore
    }
  }

  async function resolveOutputPort(): Promise<string | null> {
    if (!props.streamUuid) return null;
    if (outputPort) return outputPort;
    try {
      const raw = await StreamsApi.listPipelineOutputs({ id: props.streamUuid });
      const outputs = Array.isArray(raw) ? raw.map((entry) => entry?.name).filter((name): name is string => typeof name === 'string' && name.length > 0) : [];
      if (outputs.length === 0) return null;
      const preferred = ['detections', 'tags', 'aruco', 'markers'];
      const matched = preferred.find((key) => outputs.some((name) => name.toLowerCase() === key));
      const fallback = matched ?? outputs.find((key) => key.toLowerCase().includes('detect')) ?? outputs[0];
      outputPort = fallback ?? null;
      return outputPort;
    } catch {
      return null;
    }
  }

  async function pollOnce(): Promise<void> {
    if (!props.enabled) return;
    if (!props.streamUuid) return;
    try {
      const port = await resolveOutputPort();
      if (!port) return;
      const payload = await StreamsApi.getPipelineOutputSample({ id: props.streamUuid, port });
      const value =
        payload && typeof payload === 'object' && 'value' in payload
          ? (((payload as { value?: unknown }).value) ?? payload)
          : payload;
      const detsRaw =
        Array.isArray(value)
          ? value
          : (
              value &&
              typeof value === 'object' &&
              Array.isArray((value as { detections?: unknown }).detections)
            )
              ? ((value as { detections: unknown[] }).detections)
              : [];
      const dets = normalizeDetections(detsRaw, props.sourceResolution);
      if (dets.length) {
        lastNonEmptyDetections = dets;
        lastNonEmptyAt = Date.now();
      }
      lastDetections = dets;
      if (pendingCapture) {
        const detsForCapture = getCaptureDetections();
        updateCoverage(detsForCapture);
        updateCaptureStats(detsForCapture, true);
        lastCaptureAt = Date.now();
        pendingCapture = false;
      } else if (props.accumulateLive) {
        updateCoverage(dets);
      }
      draw();
    } catch (error) {
      const status = readErrorStatus(error);
      if (status === 400 || status === 404) {
        outputPort = null;
      }
      // ignore
    }
  }

  function stopPolling(): void {
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
    if (metricsTimer) {
      clearInterval(metricsTimer);
      metricsTimer = null;
    }
  }

  function startPolling(): void {
    stopPolling();
    void ensureStreamCapabilities();
    pollTimer = setInterval(() => void pollOnce(), 200);
    metricsTimer = setInterval(() => void pollMetricsOnce(), 1000);
    void pollOnce();
    void pollMetricsOnce();
  }

  $effect(() => {
    if (!props.enabled) {
      stopPolling();
      lastDetections = [];
      graphMs = null;
      detectMs = null;
      outputMs = null;
      draw();
      return;
    }
    startPolling();
    return () => stopPolling();
  });

  $effect(() => {
    if (!host) return;
    if (typeof window === 'undefined') return;
    ro?.disconnect();
    ro = new ResizeObserver(() => {
      setCanvasSize();
      draw();
    });
    ro.observe(host);
    setCanvasSize();
    draw();
    return () => ro?.disconnect();
  });

  $effect(() => {
    if (!canvas) return;
    ctx2d = canvas.getContext('2d');
    setCanvasSize();
    draw();
  });

  onDestroy(() => {
    stopPolling();
    ro?.disconnect();
  });
</script>

<div bind:this={host} class="pointer-events-none absolute inset-0 z-20">
  <canvas bind:this={canvas} class="absolute inset-0 h-full w-full"></canvas>
</div>
