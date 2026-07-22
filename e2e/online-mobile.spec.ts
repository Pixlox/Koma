import { devices, expect, test } from "@playwright/test";

test.use({ ...devices["iPhone 13"], browserName: "chromium" });

test("touch online reading opens, reveals controls, and exposes download", async ({
  page,
}) => {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.reload();

  await page.getByRole("button", { name: "Add" }).tap();
  await page.getByRole("menuitem", { name: "Import from link" }).tap();
  await page
    .getByLabel("Source link")
    .fill(
      "https://mangafire.to/title/70ox7-hatori-to-furuta-no-hinichijou-sahanji",
    );
  await page.getByRole("button", { name: "Check link" }).tap();
  await page.getByRole("button", { name: "Chapter", exact: true }).tap();
  await page
    .getByRole("checkbox", { name: "I have permission to download this work." })
    .check();

  const readOnline = page.getByRole("button", { name: "Read online" });
  await readOnline.tap();
  await expect(readOnline.locator(".spin")).toBeVisible();
  const reader = page.locator(".reader-shell");
  await expect(reader).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Choose chapter" })).toBeVisible();

  await page.waitForTimeout(2_800);
  await expect(reader).toHaveClass(/controls-hidden/);
  await page.touchscreen.tap(195, 420);
  await expect(reader).toHaveClass(/controls-visible/);
  await expect(
    page
      .locator(".reader-toolbar")
      .getByRole("button", { name: "Download to library" }),
  ).toBeVisible();
});
