// The setup checklist, and the first-run wizard.
import { expect, setOnboarded, test } from "./fixtures";
import { openScreen } from "./nav";

test.describe("setup", () => {
  test("the checklist step 'Choose what each camera does' opens the chooser", async ({ page }) => {
    await openScreen(page, "Start");
    // Each checklist step is a row with one button (Open, or Review once done).
    await page.locator(".step").filter({ hasText: "Choose what each camera does" }).getByRole("button").click();
    await expect(page.getByRole("tab", { name: "What cameras run" })).toBeVisible();
    await expect(page.getByRole("radiogroup", { name: "Tags" }).first()).toBeVisible();
  });

  test("the first-run wizard shows once and can be skipped", async ({ page, target }) => {
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
