// What the sim's device says, read through helios-api (`/v1`). The UI shows what the device
// reports; these checks read the device itself, after each UI action, so a UI that shows one thing
// while it deploys another fails here. Shapes: backend/src/helios/api/src/routes/{pipelines,cameras}.rs.
import { expect, type APIRequestContext } from "@playwright/test";

/** A Daedalus graph document as the device stores it (`GET /v1/pipelines/{id}`, `graph`). */
export interface DaedalusNode {
  id: string;
  label: string;
  inputs: string[];
  outputs: string[];
  const_inputs: [string, DaedalusValue][];
  metadata: Record<string, DaedalusValue>;
  [key: string]: unknown;
}
export interface DaedalusValue {
  type: string;
  value: unknown;
}
export interface DaedalusGraph {
  format: string;
  schema_version: number;
  requires: { id: string; version?: string }[];
  metadata: Record<string, unknown>;
  graph: { nodes: DaedalusNode[]; edges: unknown[]; metadata?: unknown };
}
export interface DevicePipeline {
  id: string;
  name: string;
  enabled: boolean;
  state: string;
  revision: number | null;
  graph: DaedalusGraph | null;
  bindings: Record<string, { resource_id?: string; output_width?: number; output_height?: number; pyramid?: number; source?: string }>;
  dictionary: string | null;
  search_mode: "tracked" | "full" | null;
  full_search_every: number | null;
  cpu: number | null;
  managed: boolean;
}
export interface DeviceCamera {
  id: string;
  name: string;
  settings_writable: boolean;
}
export interface CameraControl {
  name: string;
  value?: unknown;
  [key: string]: unknown;
}
export interface DeviceCameraSettings {
  writable: boolean;
  controls: CameraControl[];
  exposure_us: number | null;
  analogue_gain: number | null;
  persisted: Record<string, unknown>;
  live_error: string | null;
}

async function getJson<T>(api: APIRequestContext, path: string): Promise<T> {
  const response = await api.get(path);
  expect(response.ok(), `GET ${path}: HTTP ${response.status()}`).toBeTruthy();
  return (await response.json()) as T;
}

export const device = {
  pipelines: (api: APIRequestContext) => getJson<DevicePipeline[]>(api, "/v1/pipelines"),
  pipeline: (api: APIRequestContext, id: string) => getJson<DevicePipeline>(api, `/v1/pipelines/${encodeURIComponent(id)}`),
  cameras: (api: APIRequestContext) => getJson<DeviceCamera[]>(api, "/v1/cameras"),
  cameraSettings: (api: APIRequestContext, id: string) => getJson<DeviceCameraSettings>(api, `/v1/cameras/${encodeURIComponent(id)}/settings`),
  templates: (api: APIRequestContext) => getJson<{ id: string }[]>(api, "/v1/pipelines/templates"),
};

/** The Eidos pipelines bound to a camera: the ones the default-pipeline chooser manages. */
export function eidosPipelinesOn(pipelines: DevicePipeline[], cameraId: string): DevicePipeline[] {
  return pipelines.filter((p) => p.graph?.requires.some((r) => r.id === "eidos") && Object.values(p.bindings).some((b) => b.resource_id === cameraId));
}

/** The one enabled pipeline bound to a camera (the chooser's pipeline); fails the test otherwise. */
export async function enabledPipelineOn(api: APIRequestContext, cameraId: string): Promise<DevicePipeline> {
  const found = eidosPipelinesOn(await device.pipelines(api), cameraId).filter((p) => p.enabled);
  expect(found.length, "one enabled Eidos pipeline on the camera").toBe(1);
  return found[0];
}

/** The node types of a pipeline's graph (Daedalus `id`s, such as `eidos:mapping.live_map`). */
export function nodeTypes(p: DevicePipeline): string[] {
  return (p.graph?.graph.nodes ?? []).map((n) => n.id);
}

/** A const input's value, by node label and input name. */
export function constOf(p: DevicePipeline, label: string, input: string): DaedalusValue | undefined {
  const node = p.graph?.graph.nodes.find((n) => n.label === label);
  return node?.const_inputs.find(([name]) => name === input)?.[1];
}

/** Waits until the camera's service answers its settings (it restarts when a pipeline changes its mode,
 * and answers 503 meanwhile: the UI is right to show that, and a test should start after it). */
export async function waitForCameraService(api: APIRequestContext, cameraId: string) {
  await expect
    .poll(async () => {
      const cameras = await device.cameras(api);
      const camera = cameras.find((c) => c.id === cameraId) as (DeviceCamera & { live_error?: string | null; service_online?: boolean }) | undefined;
      return camera?.service_online === true && !camera.live_error;
    }, { timeout: 60_000 })
    .toBe(true);
}

/** The sim's camera: the one simulated sensor the sim serves (helios-peripherals' OV9782). */
export async function simCamera(api: APIRequestContext): Promise<DeviceCamera> {
  const cameras = await device.cameras(api);
  const camera = cameras.find((c) => c.name.includes("simulated"));
  expect(camera, "the sim's simulated camera is listed").toBeTruthy();
  return camera!;
}
