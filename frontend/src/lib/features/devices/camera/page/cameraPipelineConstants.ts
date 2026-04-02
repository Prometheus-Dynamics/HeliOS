import { RAW_PIPELINE_UUID as DEFAULT_RAW_PIPELINE_UUID } from '$lib/ts-bindings/runtimeContracts';

export const PIPELINE_OUTPUT_CELL_KEY = '0:0';
export const RAW_PIPELINE_ID = '__raw__';

export let RAW_PIPELINE_UUID = DEFAULT_RAW_PIPELINE_UUID;

export function setRawPipelineUuid(value: string | null | undefined): string {
  const normalized = String(value ?? '').trim().toLowerCase();
  RAW_PIPELINE_UUID = normalized.length ? normalized : DEFAULT_RAW_PIPELINE_UUID;
  return RAW_PIPELINE_UUID;
}
