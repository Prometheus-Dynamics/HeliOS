// A pipeline's settings in the Inspector: its inputs (size, pyramid), its detection (tag family,
// search) and where it runs. Each change is a click or a typed value; on the sim the device's
// binding and spec must say the same.
import { device, simCamera, type DevicePipeline } from "./device";
import { expect, test } from "./fixtures";
import { applyAprilTag, tuneRunningPipeline } from "./flows";

/** The AprilTag pipeline on the sim's camera: the one the chooser applied. */
async function runningPipeline(api: Parameters<typeof device.pipelines>[0], cameraId: string): Promise<DevicePipeline> {
  const found = (await device.pipelines(api)).filter((p) => p.enabled && Object.values(p.bindings).some((b) => b.resource_id === cameraId));
  expect(found.length, "one enabled pipeline on the camera").toBe(1);
  return found[0];
}

test.describe("pipeline settings", () => {
  test("input size and pyramid levels reach the camera binding", async ({ page, target }) => {
    await applyAprilTag(page);
    await tuneRunningPipeline(page);

    const resolution = page.getByRole("combobox", { name: "Resolution" });
    await resolution.selectOption("640x400");
    await expect(resolution).toHaveValue("640x400");

    const pyramid = page.getByRole("textbox", { name: "Pyramid levels", exact: true });
    await pyramid.click();
    await pyramid.fill("2");
    await pyramid.press("Enter");
    await expect(pyramid).toHaveValue("2");

    if (target === "sim") {
      const api = page.request;
      const camera = await simCamera(api);
      await expect
        .poll(async () => {
          const p = await runningPipeline(api, camera.id);
          const binding = Object.values(p.bindings).find((b) => b.resource_id === camera.id);
          return [binding?.output_width, binding?.output_height, binding?.pyramid];
        })
        .toEqual([640, 400, 2]);
    }
  });

  test("tag family and search mode reach the detector", async ({ page, target }) => {
    await applyAprilTag(page);
    await tuneRunningPipeline(page);

    const family = page.getByRole("combobox", { name: "Tag family" });
    await family.selectOption("apriltag_16h5");
    await expect(family).toHaveValue("apriltag_16h5");

    await page.getByRole("radio", { name: "Every frame", exact: true }).click();
    await expect(page.getByRole("radio", { name: "Every frame", exact: true })).toBeChecked();
    // Every frame has no full-search interval to set.
    await expect(page.getByRole("textbox", { name: "Full search every", exact: true })).toBeHidden();

    await page.getByRole("radio", { name: "Tracked", exact: true }).click();
    const interval = page.getByRole("textbox", { name: "Full search every", exact: true });
    await interval.click();
    await interval.fill("5");
    await interval.press("Enter");
    await expect(interval).toHaveValue("5");

    if (target === "sim") {
      const api = page.request;
      const camera = await simCamera(api);
      await expect
        .poll(async () => {
          const p = await runningPipeline(api, camera.id);
          return { dictionary: p.dictionary, search: p.search_mode, every: p.full_search_every };
        })
        .toEqual({ dictionary: "apriltag_16h5", search: "tracked", every: 5 });
    }
  });
});
