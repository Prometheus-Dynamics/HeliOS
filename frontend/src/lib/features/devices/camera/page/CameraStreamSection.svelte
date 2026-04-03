<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import { StreamMetricsPanel, StreamPreview } from '$lib';
  import type { StreamInfo } from '$lib/api/client';
  import { streamHealthStatus, streamRecordingActive } from '$lib/api/streamRuntime';
  import type { StreamMetricsError } from '$lib/api/streamMetrics';
  import StreamMetricsBanners from '$lib/components/StreamMetricsBanners.svelte';
  import { buildStreamPreviewProps } from '$lib/components/streamViewerSurface';
  import CalibrationGuidanceOverlay from '$lib/features/devices/camera/CalibrationGuidanceOverlay.svelte';

  type StreamMetricsPanelProps = ComponentProps<typeof StreamMetricsPanel>;
  type StreamBindings = {
    streamMetrics: StreamMetricsPanelProps['metrics'];
  };
  type ActiveMode = {
    format?: {
      resolution?: {
        width?: number | null;
        height?: number | null;
      } | null;
    } | null;
  } | null;
  type StreamStateCtx = {
    activeTab: string;
    streamCrop?: unknown;
    streamCropGuidesEnabled: boolean;
  };
  type PipelineStateCtx = {
    selectedPipelineId: string | null;
    selectedPipelineOutput?: string | null;
  } | null;
  type CalibrationStateCtx = {
    calibrationGuidedMode: boolean;
    calibrationGuidedResetToken: unknown;
    calibrationGuidedCaptureToken: unknown;
    calibrationGuidedAccumulateLive: boolean;
  };
  type CameraStreamSectionCtx = {
    streamBindings: StreamBindings;
    stream: StreamInfo | null;
    streamState: StreamStateCtx;
    pipelineState: PipelineStateCtx;
    activeMode: ActiveMode;
    calibrationState: CalibrationStateCtx;
    RAW_PIPELINE_ID: string;
    streamViewerHost: HTMLDivElement | null;
    streamId: string;
    outputSelectionForPipeline?: (pipelineId: string) => string | null;
  };

  const { ctx } = $props<{ ctx: CameraStreamSectionCtx }>();

  let metricsError = $state<StreamMetricsError | null>(null);
  let metricsStaleMessage = $state<string | null>(null);
  let cropGuideHostWidth = $state(0);
  let cropGuideHostHeight = $state(0);
  const recordingActive = $derived(streamRecordingActive(ctx.stream));
  const streamStatus = $derived.by(() => streamHealthStatus(ctx.stream));
  const streamAlias = $derived.by(() => {
    const identity = ctx.stream?.manifest?.identity;
    const alias = identity?.alias;
    return typeof alias === 'string' && alias.trim().length > 0 ? alias : null;
  });
  const normalizedStreamCrop = $derived.by(() => {
    const raw = Array.isArray(ctx.streamState?.streamCrop) && ctx.streamState.streamCrop.length === 4 ? ctx.streamState.streamCrop : [-1, 1, -1, 1];
    const clamp = (value: number) => Math.max(-1, Math.min(1, Number.isFinite(value) ? value : 0));
    let x0 = clamp(Number(raw[0] ?? -1));
    let x1 = clamp(Number(raw[1] ?? 1));
    let y0 = clamp(Number(raw[2] ?? -1));
    let y1 = clamp(Number(raw[3] ?? 1));
    if (x1 < x0) [x0, x1] = [x1, x0];
    if (y1 < y0) [y0, y1] = [y1, y0];
    return [x0, x1, y0, y1] as [number, number, number, number];
  });
  const cropGuideViewport = $derived.by(() => {
    const hostW = Math.max(0, Number(cropGuideHostWidth ?? 0));
    const hostH = Math.max(0, Number(cropGuideHostHeight ?? 0));
    const resolution = ctx.activeMode?.format?.resolution ?? null;
    const frameW = Math.max(0, Number(resolution?.width ?? 0));
    const frameH = Math.max(0, Number(resolution?.height ?? 0));

    if (hostW <= 0 || hostH <= 0) {
      return { x: 0, y: 0, width: 0, height: 0 };
    }
    if (frameW <= 0 || frameH <= 0) {
      return { x: 0, y: 0, width: hostW, height: hostH };
    }

    const scale = Math.min(hostW / frameW, hostH / frameH);
    const width = frameW * scale;
    const height = frameH * scale;
    const x = (hostW - width) * 0.5;
    const y = (hostH - height) * 0.5;
    return { x, y, width, height };
  });
  const streamCropRectStyle = $derived.by(() => {
    const [x0, x1, y0, y1] = normalizedStreamCrop;
    const viewport = cropGuideViewport;
    if (viewport.width <= 0 || viewport.height <= 0) {
      return 'display:none;';
    }
    const left = viewport.x + ((x0 + 1) * 0.5) * viewport.width;
    const top = viewport.y + ((y0 + 1) * 0.5) * viewport.height;
    const width = Math.max(((x1 - x0) * 0.5) * viewport.width, 1);
    const height = Math.max(((y1 - y0) * 0.5) * viewport.height, 1);
    return `left:${left}px;top:${top}px;width:${width}px;height:${height}px;`;
  });
  const undistortCalibrationWarning = $derived.by(() => {
    const manifest = ctx.stream?.manifest ?? null;
    if (!manifest) return null;
    const calib = manifest?.calibration ?? null;
    const pipelineState = ctx.pipelineState;
    // Only warn when the active viewer slot is showing the RAW stream.
    if (pipelineState?.selectedPipelineId !== ctx.RAW_PIPELINE_ID) return null;

    // Prefer the per-pipeline output selection (the dropdown writes here) so the banner reacts
    // immediately; manifest updates can lag behind.
    let outputRaw: unknown = null;
    if (typeof ctx.outputSelectionForPipeline === 'function') {
      outputRaw = ctx.outputSelectionForPipeline(ctx.RAW_PIPELINE_ID);
    }
    if (!outputRaw) outputRaw = pipelineState?.selectedPipelineOutput ?? null;
    if (!outputRaw) {
      outputRaw = typeof manifest.active_pipeline_output === 'string' ? manifest.active_pipeline_output : null;
    }
    const output = typeof outputRaw === 'string' ? outputRaw.trim().toLowerCase() : '';
    const canonical = output === 'frame' ? 'raw' : output;
    if (canonical !== 'undistorted') return null;
    if (calib) return null;
    return 'Undistorted view requires a saved calibration. You are currently seeing the raw frame.';
  });
  const previewProps = $derived.by(() =>
    buildStreamPreviewProps(
      {
        name: ctx.stream?.id ?? 'Stream',
        status: streamStatus,
        captureSessionId: ctx.stream?.id ?? ctx.streamId,
        captureSessionAlias: streamAlias,
        cameraUid: null,
        recordingActive
      },
      'device-detail'
    )
  );
</script>

<div class="flex flex-col gap-4 xl:min-h-0 xl:overflow-hidden xl:pr-1">
  <div class="flex flex-col overflow-hidden rounded border border-surface-800/70 bg-surface-950 xl:flex-1 xl:min-h-0">
    <div class="relative flex items-center justify-center bg-black/95 aspect-video xl:aspect-auto xl:flex-1 xl:min-h-0">
        <div bind:this={ctx.streamViewerHost} class="absolute inset-0 flex items-center justify-center p-2">
          <div class="h-full w-full">
          <StreamPreview {...previewProps}>
            {#if ctx.streamState.activeTab === 'calibration' && ctx.calibrationState.calibrationGuidedMode && ctx.stream?.id}
              <CalibrationGuidanceOverlay
                enabled={ctx.calibrationState.calibrationGuidedMode}
                streamUuid={ctx.stream.id}
                sourceResolution={ctx.activeMode?.format?.resolution ?? null}
                resetToken={ctx.calibrationState.calibrationGuidedResetToken}
                captureToken={ctx.calibrationState.calibrationGuidedCaptureToken}
                accumulateLive={ctx.calibrationState.calibrationGuidedAccumulateLive}
                fitMode="contain"
                drawOverlay={true}
              />
            {/if}
            {#if ctx.streamState.streamCropGuidesEnabled}
              <div class="pointer-events-none absolute inset-0 z-10">
                <div
                  class="absolute inset-0 overflow-hidden"
                  bind:clientWidth={cropGuideHostWidth}
                  bind:clientHeight={cropGuideHostHeight}
                >
                  <div
                    class="absolute rounded-[2px] border border-dashed border-primary-300/90"
                    style={`${streamCropRectStyle}box-shadow: 0 0 0 9999px rgba(2, 6, 23, 0.3);`}
                  ></div>
                </div>
              </div>
            {/if}
            {#if metricsError || metricsStaleMessage}
              <div class="pointer-events-none absolute inset-x-3 bottom-3 z-20 flex flex-col gap-2">
                <div class="pointer-events-auto">
                  <StreamMetricsBanners error={metricsError} metricsStaleMessage={metricsStaleMessage} />
                </div>
              </div>
            {/if}
            {#if undistortCalibrationWarning}
              <div class="pointer-events-none absolute inset-x-3 top-3 z-20">
                <div class="pointer-events-auto rounded border border-warning-800/60 bg-warning-950/30 px-3 py-2 text-xs text-warning-100">
                  {undistortCalibrationWarning}
                </div>
              </div>
            {/if}
          </StreamPreview>
        </div>
      </div>
    </div>
  </div>

  <StreamMetricsPanel
    captureSessionId={ctx.streamId}
    captureSessionAlias={streamAlias}
    showHeader={false}
    hideHostMetrics
    showBanners={false}
    compact={true}
    summaryBar={true}
    bind:metricsError={metricsError}
    bind:metricsStaleMessage={metricsStaleMessage}
    bind:metrics={ctx.streamBindings.streamMetrics}
  />
</div>
