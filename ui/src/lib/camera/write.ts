// How the camera fields write to the device. One place, so the pane and the inspector send the
// same keys: a control's standard key when it has one (`exposure_us`, `ae`, `fps`...), else its
// name. The live store batches writes made while dragging into one PATCH; the mock applies them.
import type { Camera, CameraControlInfo } from "#lib/api/model.js";
import { cluster } from "#lib/stores/cluster.svelte.js";

export const keyOf = (control: Pick<CameraControlInfo, "name" | "standard">): string => control.standard ?? control.name;

export const controlNamed = (controls: CameraControlInfo[], name: string): CameraControlInfo | undefined => controls.find((c) => c.name === name);

/** Set controls on a camera, by key (see `keyOf`), in one write. */
export function writeControls(camera: Camera, changes: Record<string, number | boolean>) {
  cluster.setCamera(camera.resourceId, {}, changes);
}
