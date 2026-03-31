import { resolveStreamCreationDefaults } from '$lib/api/streamDefaults';
import type { StreamCapabilitiesResponse, StreamManifest } from '$lib/ts-bindings/http/client';

export type RegisterNetcamStreamInput = {
  url: string;
  name?: string | null;
  fps?: number | null;
  width?: number | null;
  height?: number | null;
  startOnBoot?: boolean | null;
};

export function makeNetcamManifest(input: RegisterNetcamStreamInput, capabilities: StreamCapabilitiesResponse): StreamManifest {
  const url = input.url.trim();
  const fps = typeof input.fps === 'number' && Number.isFinite(input.fps) ? Math.max(1, Math.min(120, Math.trunc(input.fps))) : 30;
  const width = typeof input.width === 'number' && Number.isFinite(input.width) ? Math.max(0, Math.trunc(input.width)) : 0;
  const height = typeof input.height === 'number' && Number.isFinite(input.height) ? Math.max(0, Math.trunc(input.height)) : 0;
  const alias = input.name?.trim().length ? input.name.trim() : `Netcam ${url}`;
  const streamDefaults = resolveStreamCreationDefaults(capabilities);
  const rawPipelineId = String(capabilities.rawPipelineId ?? '').trim();
  const rawOutput = streamDefaults?.rawOutput ?? '';
  if (!streamDefaults || !rawPipelineId || !rawOutput) {
    throw new Error('Stream capabilities missing requested stream defaults');
  }

  return {
    identity: { id: null, alias, hardware_id: null } as unknown as StreamManifest['identity'],
    capture: {
      device_keys: [url],
      backend: 'Netcam',
      handle: {
        type: 'netcam',
        url,
        width,
        height,
        fps
      },
      mode: {
        format: {
          code: 'MJPG',
          color: 'Srgb',
          resolution: { width: Math.max(1, width || 1), height: Math.max(1, height || 1) }
        },
        interval: { numerator: 1, denominator: fps }
      },
      interval: { numerator: 1, denominator: fps },
      controls: []
    },
    host_buffer: streamDefaults.defaultHostBuffer,
    preview_jpeg_quality: streamDefaults.defaultPreviewJpegQuality,
    pipeline_enabled: streamDefaults.pipelineEnabledWhenBindingsPresent,
    active_pipeline_id: rawPipelineId,
    active_pipeline_output: rawOutput,
    pipelines: [{ pipeline_id: rawPipelineId, pipeline_graph: null, pipeline_output: rawOutput }],
    pipeline_layout: {
      rows: 1,
      columns: 1,
      slots: [{ row: 0, column: 0, pipeline_id: rawPipelineId, output_key: rawOutput }]
    },
    shadow_recorder_enabled: streamDefaults.defaultShadowRecorderEnabled,
    start_on_boot: input.startOnBoot == null ? streamDefaults.defaultStartOnBoot : Boolean(input.startOnBoot)
  } as unknown as StreamManifest;
}
