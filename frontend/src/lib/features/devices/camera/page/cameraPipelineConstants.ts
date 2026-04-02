export const PIPELINE_OUTPUT_CELL_KEY = '0:0';
export const RAW_PIPELINE_ID = '__raw__';

const DEFAULT_RAW_PIPELINE_UUID = '00000000-0000-0000-0000-0000000000aa';

export let RAW_PIPELINE_UUID = DEFAULT_RAW_PIPELINE_UUID;

export function setRawPipelineUuid(value: string | null | undefined): string {
  const normalized = String(value ?? '').trim().toLowerCase();
  RAW_PIPELINE_UUID = normalized.length ? normalized : DEFAULT_RAW_PIPELINE_UUID;
  return RAW_PIPELINE_UUID;
}
