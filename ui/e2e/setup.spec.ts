// The setup checklist (each camera's status chain as its steps), and the first-run wizard.
import { expect, setOnboarded, test } from "./fixtures";
import { openScreen, tab } from "./nav";

test.describe("setup", () => {
  test("a camera's next step opens the pane that does it, with the camera selected", async ({ page }) => {
    await openScreen(page, "Set up");
    // Every camera is a section with its steps; the step it is at has the one Open button.
    const first = page.locator("section.cam").first();
    await expect(first).toBeVisible();
    const step = first.locator(".step.current");
    await expect(step).toHaveCount(1);
    const id = await step.getAttribute("data-step");
    await step.getByRole("button", { name: "Open" }).click();
    // The pane for the step is in front: the pipeline chooser for a pipeline, calibration, mounts.
    const pane = { pipeline: "Cameras", detections: "Camera controls", calibration: "Calibration", place: "Mounts" }[id ?? ""] ?? "Cameras";
    await expect(tab(page, pane).first()).toHaveAttribute("aria-selected", "true");
  });

  test("Set up > Cameras has each camera's chooser", async ({ page }) => {
    await openScreen(page, "Set up");
    await tab(page, "Cameras").click();
    await expect(page.getByRole("radiogroup", { name: "Tags" }).first()).toBeVisible();
  });

  test("the first-run wizard shows once, asks the team, and lands on Set up", async ({ page, target }) => {
    // Setup not done on the device, and the fixture stops seeding it again: the wizard is what this tests.
    await page.evaluate(() => sessionStorage.setItem("e2e:no-onboarding-seed", "1"));
    await setOnboarded(page, target, false);
    await page.reload();
    const wizard = page.getByRole("dialog", { name: "Set up HeliOS" });
    await expect(wizard).toBeVisible();
    await page.getByRole("button", { name: "Skip setup" }).click();
    await expect(wizard).toBeHidden();
    await page.reload();
    await expect(page.getByRole("navigation", { name: "Screens" })).toBeVisible();
    await expect(wizard).toBeHidden();
  });
});
