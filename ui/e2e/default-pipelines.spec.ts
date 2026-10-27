// Default pipelines: a camera's one pipeline, picked on Set up > Cameras.
// The UI's switches and choices are real clicks; on the sim, every step is checked on the device.
import { device, eidosPipelinesOn, enabledPipelineOn, nodeTypes, simCamera } from "./device";
import { expect, test } from "./fixtures";
import { applyIfChanged } from "./flows";
import { firstCameraChooser, openPane } from "./nav";

test.describe("default pipelines", () => {
  test("AprilTag with 3D map and depth, then ArUco, then video only", async ({ page, target }) => {
    await openPane(page, "Set up", "Cameras");

    const chooser = firstCameraChooser(page);
    await expect(chooser).toBeVisible();

    // AprilTag, with the 3D map on, then depth: each switch changes once per click.
    await chooser.getByRole("radio", { name: "AprilTag" }).click();
    const map = chooser.getByRole("switch", { name: "3D map" });
    const depth = chooser.getByRole("switch", { name: "Depth" });
    // Start from a known choice: whatever the camera ran before, the map and depth are off.
    if (await depth.isChecked()) {
      await depth.click();
      await expect(depth).not.toBeChecked();
    }
    if (await map.isChecked()) {
      await map.click();
      await expect(map).not.toBeChecked();
    }
    await map.click();
    await expect(map).toBeChecked();
    await depth.click();
    await expect(depth).toBeChecked();
    await expect(chooser.getByText("Localization, 3D map and depth", { exact: false })).toBeVisible();

    await applyIfChanged(chooser);

    if (target === "sim") {
      const api = page.request;
      const camera = await simCamera(api);
      const firstId = (await enabledPipelineOn(api, camera.id)).id;
      await expect
        .poll(async () => nodeTypes(await device.pipeline(api, firstId)))
        .toEqual(expect.arrayContaining(["eidos:mapping.live_map", "eidos:dense.map"]));
      // The camera's tag family stays across a template change (the chooser keeps it).
      expect((await device.pipeline(api, firstId)).dictionary).toMatch(/^apriltag_/);

      // ArUco replaces the same pipeline: same id, so robot code's NetworkTables path holds.
      await chooser.getByRole("radio", { name: "ArUco" }).click();
      await applyIfChanged(chooser);
      await expect
        .poll(async () => {
          const p = await device.pipeline(api, firstId);
          return { map: nodeTypes(p).includes("eidos:mapping.live_map"), family: p.dictionary?.split("_")[0] };
        })
        .toEqual({ map: false, family: "aruco" });
      await expect
        .poll(async () => eidosPipelinesOn(await device.pipelines(api), camera.id).filter((p) => p.enabled).map((p) => p.id))
        .toEqual([firstId]);

      // None is video only: the camera's Eidos pipelines are all stopped.
      await chooser.getByRole("radio", { name: "None" }).click();
      await applyIfChanged(chooser);
      await expect.poll(async () => eidosPipelinesOn(await device.pipelines(api), camera.id).filter((p) => p.enabled).length).toBe(0);
      expect((await device.pipeline(api, firstId)).enabled).toBe(false);
    } else {
      // The mock has no device: the chooser's own text follows each choice.
      await chooser.getByRole("radio", { name: "ArUco" }).click();
      await expect(chooser.getByText("ArUco markers give each marker's pose")).toBeVisible();
      await applyIfChanged(chooser);
      await chooser.getByRole("radio", { name: "None" }).click();
      await expect(chooser.getByText("Video only", { exact: true })).toBeVisible();
      await applyIfChanged(chooser);
      await expect(chooser.getByRole("button", { name: "Apply", exact: true })).toBeHidden();
    }
  });
});
