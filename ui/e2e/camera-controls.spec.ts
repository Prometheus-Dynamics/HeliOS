// Camera controls: search, "Changed only", exposure and gain, and reset. On the sim each change is
// read back from the camera's settings (GET /v1/cameras/{id}/settings).
import { device, simCamera, waitForCameraService } from "./device";
import { expect, test } from "./fixtures";
import { openScreen } from "./nav";

test.describe("camera controls", () => {
  // The camera service restarts when a control changes its mode, and answers its settings with 503
  // until it is back. The UI shows that as the camera's live error; it is the one console error allowed.
  test.use({ allowConsole: [/status of 503 .*\/v1\/cameras\/[^/]+\/settings/] });

  test("search filters, changed-only shows what changed, exposure and gain reach the camera, reset", async ({ page, target }) => {
    await openScreen(page, "Cameras");
    const api = page.request;
    const camera = target === "sim" ? await simCamera(api) : null;
    const reset = page.getByRole("button", { name: "Reset all controls to defaults" });
    const search = page.getByRole("textbox", { name: "Search camera controls" });
    const changedOnly = page.getByRole("button", { name: "Changed only", exact: true });
    const exposure = page.getByRole("textbox", { name: "Exposure", exact: true }).first();
    const gain = page.getByRole("textbox", { name: "Gain", exact: true }).first();

    // Start from the defaults, so "changed" means what this test changed.
    if (camera) await waitForCameraService(api, camera.id);
    await reset.click();
    if (camera) await expect.poll(async () => (await device.cameraSettings(api, camera.id)).persisted).toEqual({});

    // Exposure is typed in manual mode: auto exposure holds the field.
    await page.getByRole("radiogroup", { name: "Exposure mode" }).getByRole("radio", { name: "Manual", exact: true }).click();
    await expect(exposure).toBeEnabled();

    // Search narrows the sections: gain is in the exposure section, so that stays, and white balance goes.
    await search.fill("gain");
    await expect(page.getByRole("slider", { name: "Gain", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "White balance", exact: true })).toBeHidden();
    await expect(page.getByRole("button", { name: "Flicker", exact: true })).toBeHidden();
    await search.fill("");
    await expect(page.getByRole("slider", { name: "Exposure", exact: true })).toBeVisible();

    // Exposure in ms and gain as a factor, typed.
    await exposure.click();
    await exposure.fill("3.00");
    await exposure.press("Enter");
    await expect(exposure).toHaveValue("3.00");
    await gain.click();
    await gain.fill("2.00");
    await gain.press("Enter");
    await expect(gain).toHaveValue("2.00");

    // Changed only: the two controls are shown, and the unchanged ones are not.
    await changedOnly.click();
    await expect(changedOnly).toHaveAttribute("aria-pressed", "true");
    await expect(exposure).toBeVisible();
    await expect(gain).toBeVisible();
    // White balance has nothing changed, so its section goes.
    await expect(page.getByRole("button", { name: "White balance", exact: true })).toBeHidden();
    await changedOnly.click();
    await expect(changedOnly).toHaveAttribute("aria-pressed", "false");

    if (camera) {
      // The sensor quantises its exposure line time: 3.00 ms reads back within a few microseconds.
      await expect
        .poll(async () => {
          const s = await device.cameraSettings(api, camera.id);
          return { exposureNearOk: Math.abs((s.exposure_us ?? -1e9) - 3000) <= 30, gainOk: Math.abs((s.analogue_gain ?? -1) - 2) <= 0.01 };
        })
        .toEqual({ exposureNearOk: true, gainOk: true });
    }

    // Reset puts the defaults back on the camera and forgets what was kept.
    await reset.click();
    await expect(exposure).not.toHaveValue("3.00");
    if (camera) {
      await expect.poll(async () => (await device.cameraSettings(api, camera.id)).persisted).toEqual({});
      await expect.poll(async () => Math.round((await device.cameraSettings(api, camera.id)).exposure_us ?? -1)).not.toBe(3000);
    }
  });
});
