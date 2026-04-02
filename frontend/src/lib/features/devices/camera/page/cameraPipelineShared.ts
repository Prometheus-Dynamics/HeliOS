export const PIPELINE_UI_STORAGE_PREFIX = 'helios.camera.pipelineUi.v1.';
export const PIPELINE_OUTPUT_CELL_KEY = '0:0';
export const RAW_PIPELINE_ID = '__raw__';
const DEFAULT_RAW_PIPELINE_UUID = '00000000-0000-0000-0000-0000000000aa';
export let RAW_PIPELINE_UUID = DEFAULT_RAW_PIPELINE_UUID;

export function setRawPipelineUuid(value: string | null | undefined): string {
  const normalized = String(value ?? '').trim().toLowerCase();
  RAW_PIPELINE_UUID = normalized.length ? normalized : DEFAULT_RAW_PIPELINE_UUID;
  return RAW_PIPELINE_UUID;
}

export const RAW_LOOPBACK_GRAPH = {
  nodes: [
    {
      id: 'io.host_bridge',
      label: 'Input:frame+calibration',
      inputs: [],
      outputs: ['frame', 'calibration'],
      metadata: { host_bridge: { type: 'Bool', value: true } }
    },
    {
      id: 'cv:image:undistort_optional',
      label: 'Undistort (calibration)',
      inputs: ['frame', 'calibration', 'border_mode', 'zoom_mode', 'zoom', 'fill_margin'],
      outputs: ['frame'],
      const_inputs: [
        ['border_mode', { type: 'String', value: 'zero' }],
        ['zoom_mode', { type: 'String', value: 'fill' }],
        ['fill_margin', { type: 'Float', value: 1.0 }]
      ]
    },
    {
      id: 'io.host_output',
      label: 'Output:raw+undistorted',
      inputs: ['frame', 'raw', 'undistorted'],
      outputs: [],
      metadata: { host_bridge: { type: 'Bool', value: true } }
    }
  ],
  edges: [
    { from: { node: 0, port: 'frame' }, to: { node: 2, port: 'frame' } },
    { from: { node: 0, port: 'frame' }, to: { node: 2, port: 'raw' } },
    { from: { node: 0, port: 'frame' }, to: { node: 1, port: 'frame' } },
    { from: { node: 0, port: 'calibration' }, to: { node: 1, port: 'calibration' } },
    { from: { node: 1, port: 'frame' }, to: { node: 2, port: 'undistorted' } }
  ],
  metadata: {}
};

export function normalizeAssignedPipelineIds(ids: string[]): string[] {
  return Array.from(
    new Set(
      (ids ?? [])
        .map((id) => String(id ?? '').trim())
        .filter((id) => id.length && id !== RAW_PIPELINE_ID && id !== RAW_PIPELINE_UUID)
    )
  ).slice(0, 64);
}

export function normalizePipelineOutputMap(map: Record<string, string | null> | null | undefined): Record<string, string | null> {
  const out: Record<string, string | null> = {};
  if (!map || typeof map !== 'object') return out;
  for (const [rawKey, rawValue] of Object.entries(map)) {
    const key = String(rawKey ?? '').trim();
    if (!key.length) continue;
    if (key === RAW_PIPELINE_ID) continue;
    const value = typeof rawValue === 'string' ? rawValue.trim() : '';
    out[key] = value.length ? value : null;
  }
  return out;
}
