// Camera controls: search, "Changed only", exposure and gain, and reset. On the sim each change is
// read back from the camera's settings (GET /v1/cameras/{id}/settings).
import { device, readingOf, simCamera, waitForCameraService } from "./device";
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

    // What the device reports for exposure and gain, by their roles (whatever the camera calls them).
    const reading = async (role: "exposure_us" | "gain") => (camera ? readingOf(await device.cameraSettings(api, camera.id), role) : null);

    // Start from the defaults, so "changed" means what this test changed.
    if (camera) await waitForCameraService(api, camera.id);
    await reset.click();
    if (camera) await expect.poll(async () => (await device.cameraSettings(api, camera.id)).persisted).toEqual({});
    const exposureAtStart = await reading("exposure_us");

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

    // Exposure in ms and gain as a factor, typed. 1.20 ms fits under every cap a camera may have
    // (the AprilTag preset caps exposure at 1.5 ms on a real board), so the camera takes it as typed.
    await exposure.click();
    await exposure.fill("1.20");
    await exposure.press("Enter");
    // The field shows short exposures with more decimals ("1.200"): compare the number.
    await expect.poll(async () => Math.abs(parseFloat(await exposure.inputValue()) - 1.2) < 0.005).toBe(true);
    // Gain is a factor on the libcamera devices (two decimals) and a whole number on a UVC camera.
    await gain.click();
    await gain.fill("2.00");
    await gain.press("Enter");
    await expect.poll(async () => Math.abs(parseFloat(await gain.inputValue()) - 2) < 0.005).toBe(true);

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
      // The sensor quantises its exposure line time: 1.20 ms reads back within a few microseconds where
      // the device reports µs (its summary). A camera without that summary reports its own units, so
      // there the exposure only has to have moved from where it started.
      await expect
        .poll(async () => {
          const s = await device.cameraSettings(api, camera.id);
          return {
            exposureOk: s.exposure_us !== null ? Math.abs(s.exposure_us - 1200) <= 30 : readingOf(s, "exposure_us") !== exposureAtStart,
            gainOk: Math.abs((readingOf(s, "gain") ?? -1) - 2) <= 0.01,
          };
        })
        .toEqual({ exposureOk: true, gainOk: true });
    }

    // Reset puts the defaults back on the camera and forgets what was kept.
    await reset.click();
    await expect.poll(async () => Math.abs(parseFloat(await exposure.inputValue()) - 1.2) < 0.005).toBe(false);
    if (camera) {
      await expect.poll(async () => (await device.cameraSettings(api, camera.id)).persisted).toEqual({});
      await expect.poll(() => reading("exposure_us")).toBe(exposureAtStart);
    }
  });
});
