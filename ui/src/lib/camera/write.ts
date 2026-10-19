// How the camera fields write to the device. One place, so the pane and the inspector send the
// same keys: a control's standard key when it has one (`exposure_us`, `ae`, `fps`...), else its
// name. The live store batches writes made while dragging into one PATCH; the mock applies them.
// A `null` forgets a control: it goes to the device's default and the value kept for it is dropped.
import type { Camera, CameraControlInfo, CropRect } from "#lib/api/model.js";
import { cluster } from "#lib/stores/cluster.svelte.js";

export const keyOf = (control: Pick<CameraControlInfo, "name" | "standard">): string => control.standard ?? control.name;

export const controlNamed = (controls: CameraControlInfo[], name: string): CameraControlInfo | undefined => controls.find((c) => c.name === name);

/** Set controls on a camera, by key (see `keyOf`), in one write. A rectangle sets a region control. */
export function writeControls(camera: Camera, changes: Record<string, number | boolean | CropRect | null>) {
  cluster.setCamera(camera.resourceId, {}, changes);
}

/** Put a control back to its default and forget the value kept for it (its reset). */
export function forgetControl(camera: Camera, control: Pick<CameraControlInfo, "name" | "standard">) {
  writeControls(camera, { [keyOf(control)]: null });
}
