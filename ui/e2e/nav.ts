// Navigation helpers: the rail's screens, by the names a user sees.
import { expect, type Page } from "@playwright/test";

/** Every screen in the left rail, in order (the built-in presets). */
export const SCREENS = ["Start", "Overview", "Cameras", "Vision", "Pipelines", "Robot", "Calibration", "Recordings", "Hardware", "Playback", "Settings"] as const;
export type Screen = (typeof SCREENS)[number];

/** Opens a screen with a click on its rail button. */
export async function openScreen(page: Page, screen: Screen) {
  await page.getByRole("navigation", { name: "Screens" }).getByRole("button", { name: screen, exact: true }).click();
  await expect(page.getByRole("navigation", { name: "Screens" }).getByRole("button", { name: screen, exact: true })).toHaveAttribute("class", /active/);
}

/** The camera section of the "What cameras run" list (or the first one): where a camera's chooser is. */
export function firstCameraChooser(page: Page) {
  return page.locator("section").filter({ has: page.getByRole("radiogroup", { name: "Tags" }) }).first();
}
