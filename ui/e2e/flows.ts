// Steps several flows share, each done with clicks in the UI.
import { expect, type Locator, type Page } from "@playwright/test";
import { firstCameraChooser, openPane, tab } from "./nav";

/**
 * Clicks the chooser's Apply when it shows (it shows only when the choice differs from what runs),
 * then waits until the write is done. The chooser keeps its choice (Cancel shows) until the apply
 * returns, so Cancel gone means the device has the write; a check made earlier reads too soon.
 */
export async function applyIfChanged(chooser: Locator) {
  const apply = chooser.getByRole("button", { name: "Apply", exact: true });
  if (await apply.isVisible()) await apply.click();
  await expect(chooser.getByRole("button", { name: "Cancel", exact: true })).toBeHidden();
}

/**
 * Set up > Cameras -> the first camera's chooser: AprilTag localization, applied. `map` and `depth`
 * set those switches when given and otherwise leave them as the camera has them. Leaves the
 * chooser showing the camera's running pipeline.
 */
export async function applyAprilTag(page: Page, { map, depth }: { map?: boolean; depth?: boolean } = {}) {
  await openPane(page, "Set up", "Cameras");
  const chooser = firstCameraChooser(page);
  await chooser.getByRole("radio", { name: "AprilTag" }).click();
  const map3d = chooser.getByRole("switch", { name: "3D map" });
  if (map !== undefined && (await map3d.isChecked()) !== map) await map3d.click();
  const depthSwitch = chooser.getByRole("switch", { name: "Depth" });
  if (depth !== undefined && (await depthSwitch.isChecked()) !== depth) await depthSwitch.click();
  await applyIfChanged(chooser);
  return chooser;
}

/** From the chooser's "Tune pipeline" link: the pipeline's settings, on Tune. */
export async function tuneRunningPipeline(page: Page) {
  await page.getByRole("button", { name: "Tune pipeline" }).first().click();
  await expect(page.getByRole("navigation", { name: "Screens" }).getByRole("button", { name: "Tune", exact: true })).toHaveAttribute("class", /active/);
  await expect(tab(page, "Pipeline settings")).toHaveAttribute("aria-selected", "true");
}
