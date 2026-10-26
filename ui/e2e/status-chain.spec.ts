// The status chain: each camera's stage that stops it, and each pipeline's reason, shown where the
// operator looks. The simulated robot (`?mock=1`) has one scenario per way a camera gets stuck
// (api/mock.ts); on a real device the same text comes from GET /v1/cameras and GET /v1/pipelines.
import { expect, test } from "./fixtures";
import { openScreen } from "./nav";

test.describe("status chain", () => {
  test("the camera list says which stage stops each camera", async ({ page, target }) => {
    test.skip(target !== "mock", "the scenarios are the simulated robot's");
    await openScreen(page, "Cameras");
    // A camera another program holds: the frames stage, and the program.
    await expect(page.getByText("In use by visionserver (Limelight vision)").first()).toBeVisible();
    // A pipeline that decodes another tag family than the field's.
    await expect(page.getByText("No tags: the pipeline decodes apriltag_16h5, and the field's tags are apriltag_36h11").first()).toBeVisible();
    // The live map fitting its lens: how far it is toward its window.
    await expect(page.getByText("Self-calibrating: 40 of 150 frames, 4 views so far").first()).toBeVisible();
    // The map tracks, and the dense mapper has no depth until the camera moves.
    await expect(page.getByText("No depth yet: 2 keyframes").first()).toBeVisible();
  });

  test("the pipelines list says why a pipeline is not running", async ({ page, target }) => {
    test.skip(target !== "mock", "the scenarios are the simulated robot's");
    await openScreen(page, "Pipelines");
    await expect(page.getByText("Failed: the CLAHE pre-filter raised an error").first()).toBeVisible();
    await expect(page.getByText("quarantined · 5 restarts")).toHaveCount(0);
  });
});
