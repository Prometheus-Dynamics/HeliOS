// Navigation helpers: the rail's jobs and their panes, by the names a user sees.
import { expect, type Page } from "@playwright/test";

/** The four jobs in the left rail, in order (docs/docs/ui-information-architecture.md). */
export const SCREENS = ["Set up", "Run", "Tune", "Device"] as const;
export type Screen = (typeof SCREENS)[number];

/** Opens a job with a click on its rail button. */
export async function openScreen(page: Page, screen: Screen) {
  await page.getByRole("navigation", { name: "Screens" }).getByRole("button", { name: screen, exact: true }).click();
  await expect(page.getByRole("navigation", { name: "Screens" }).getByRole("button", { name: screen, exact: true })).toHaveAttribute("class", /active/);
}

/** A pane's tab, by its title (the tab's name also carries its close button's label). */
export function tab(page: Page, pane: string) {
  const title = pane.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return page.getByRole("tab", { name: new RegExp(`^${title}( Close ${title})?$`) });
}

/** Opens a job and brings one of its panes to the front with a click on its tab. */
export async function openPane(page: Page, screen: Screen, pane: string) {
  await openScreen(page, screen);
  const t = tab(page, pane).first();
  await t.click();
  await expect(t).toHaveAttribute("aria-selected", "true");
}

/** The camera section of Set up > Cameras (or the first one): where a camera's chooser is. */
export function firstCameraChooser(page: Page) {
  return page.locator("section").filter({ has: page.getByRole("radiogroup", { name: "Tags" }) }).first();
}
