import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

async function openCleanPreview(page: Page) {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await expect(
    page.getByRole("button", { name: "Continue reading" }),
  ).toBeVisible();
}

async function evidence(page: Page, name: string) {
  if (process.env.KOMA_WRITE_EVIDENCE !== "1") return;
  await page.screenshot({
    path: `docs/release-evidence/screenshots/${name}.png`,
    fullPage: true,
  });
}

async function expectNoAccessibilityViolations(page: Page, surface: string) {
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();
  const summary = results.violations
    .map(
      (violation) =>
        `${violation.id}: ${violation.help}\n${violation.nodes
          .map((node) => `  ${node.target.join(" ")}: ${node.failureSummary ?? ""}`)
          .join("\n")}`,
    )
    .join("\n");
  expect(
    results.violations,
    `${surface} has automated WCAG A/AA violations:\n${summary}`,
  ).toEqual([]);
}

test("desktop library, command search, and reader are operational", async ({
  page,
}) => {
  await openCleanPreview(page);
  await expect(page.getByRole("heading", { name: "Recently added" })).toBeVisible();
  await expectNoAccessibilityViolations(page, "Desktop library");
  await evidence(page, "library-desktop");

  await page.keyboard.press("Control+k");
  await page.getByRole("textbox", { name: "Search commands" }).fill("favorites");
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("heading", { name: "Favorites", level: 2 }),
  ).toBeVisible();

  await page.getByRole("button", { name: "Home" }).first().click();
  await page.getByRole("button", { name: "Continue reading" }).click();
  await expect(page.getByLabel("Reading After the Last Train")).toBeVisible();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("slider", { name: "Page" })).toHaveAttribute(
    "aria-valuenow",
    "19",
  );
  await evidence(page, "reader-desktop");

  await page.keyboard.press("s");
  await expect(
    page.getByRole("heading", { name: "Reader settings" }),
  ).toBeVisible();
  await page.getByLabel("Label").fill("Visual evidence");
  await page.getByLabel("Note").fill("Reader notes survive with the page.");
  await page.getByRole("button", { name: "Save note" }).click();
  await expect(page.getByText("Page note saved")).toBeVisible();
  await expectNoAccessibilityViolations(page, "Reader settings");
  await evidence(page, "reader-settings-desktop");
});

test("link import blocks download until explicit permission confirmation", async ({
  page,
}) => {
  await openCleanPreview(page);
  await page.getByRole("button", { name: "Import from link" }).click();
  await page
    .getByLabel("Source link")
    .fill(
      "https://mangafire.to/title/70ox7-hatori-to-furuta-no-hinichijou-sahanji/volume/339405",
    );
  await page.getByRole("button", { name: "Check link" }).click();
  await expect(page.getByText("MangaFire")).toBeVisible();
  await expect(page.getByText("Volume 1 · Latin American Spanish")).toBeVisible();
  await page.getByRole("button", { name: "Chapter", exact: true }).click();
  await expect(page.getByRole("combobox", { name: "Choose chapter" })).toHaveValue(
    "17",
  );
  await expect(page.locator(".chapter-picker > span")).toContainText("19 pages");

  await page.getByRole("button", { name: "Entire series" }).click();
  const seriesSummary = page.locator(".series-import-summary");
  const seriesTitle = seriesSummary.getByText("Earliest to latest");
  const seriesCount = seriesSummary.getByText("17 chapters");
  const seriesPages = seriesSummary.getByText("435 pages");
  await expect(seriesTitle).toBeVisible();
  await expect(seriesCount).toBeVisible();
  await expect(seriesPages).toBeVisible();
  const titleBounds = await seriesTitle.boundingBox();
  const countBounds = await seriesCount.boundingBox();
  const pageBounds = await seriesPages.boundingBox();
  expect(titleBounds).not.toBeNull();
  expect(countBounds).not.toBeNull();
  expect(pageBounds).not.toBeNull();
  if (titleBounds !== null && countBounds !== null && pageBounds !== null) {
    expect(countBounds.x).toBeGreaterThanOrEqual(
      titleBounds.x + titleBounds.width,
    );
    expect(pageBounds.y).toBeGreaterThanOrEqual(
      Math.max(
        titleBounds.y + titleBounds.height,
        countBounds.y + countBounds.height,
      ),
    );
  }
  await evidence(page, "import-series");
  const download = page.getByRole("button", {
    name: "Download and add to Koma",
  });
  await expect(download).toBeDisabled();
  await expectNoAccessibilityViolations(page, "Link importer");
  await evidence(page, "import-permission");

  await page
    .getByRole("checkbox", {
      name: "I have permission to download this work.",
    })
    .check();
  await expect(download).toBeEnabled();
});

test("reader sliders, mode arrow, and page motion remain interactive", async ({
  page,
}) => {
  await openCleanPreview(page);
  await page.getByRole("button", { name: "Continue reading" }).click();

  const pageSlider = page.getByRole("slider", { name: "Page" });
  const progressTrack = page.locator(".reader-page-slider");
  const originalPage = Number(await pageSlider.getAttribute("aria-valuenow"));
  const pageThumbBounds = await pageSlider.boundingBox();
  const progressBounds = await progressTrack.boundingBox();
  expect(pageThumbBounds).not.toBeNull();
  expect(progressBounds).not.toBeNull();
  if (pageThumbBounds === null || progressBounds === null) return;
  await page.mouse.move(
    pageThumbBounds.x + pageThumbBounds.width / 2,
    pageThumbBounds.y + pageThumbBounds.height / 2,
  );
  await page.mouse.down();
  await page.mouse.move(
    progressBounds.x + progressBounds.width * 0.9,
    progressBounds.y + progressBounds.height / 2,
  );
  await page.mouse.up();
  await expect(pageSlider).not.toHaveAttribute(
    "aria-valuenow",
    String(originalPage),
  );

  const modeControl = page.locator(".reader-mode-button");
  const modeSelect = page.getByRole("combobox", { name: "Reading mode" });
  const originalMode = await modeSelect.inputValue();
  const modeBounds = await modeControl.boundingBox();
  const selectBounds = await modeSelect.boundingBox();
  expect(modeBounds).not.toBeNull();
  expect(selectBounds).not.toBeNull();
  if (modeBounds === null || selectBounds === null) return;
  const arrowX = modeBounds.x + modeBounds.width - 8;
  const arrowY = modeBounds.y + modeBounds.height / 2;
  expect(arrowX).toBeGreaterThanOrEqual(selectBounds.x);
  expect(arrowX).toBeLessThanOrEqual(selectBounds.x + selectBounds.width);
  expect(arrowY).toBeGreaterThanOrEqual(selectBounds.y);
  expect(arrowY).toBeLessThanOrEqual(selectBounds.y + selectBounds.height);
  await page.mouse.click(
    arrowX,
    arrowY,
  );
  await expect(modeSelect).toBeFocused();
  await modeSelect.selectOption(
    originalMode === "spreads" ? "singlePage" : "spreads",
  );
  await expect(modeSelect).not.toHaveValue(originalMode);

  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Reader settings", exact: true }).click();
  const gamma = page.getByRole("slider", { name: "Gamma" });
  await gamma.scrollIntoViewIfNeeded();
  const gammaRoot = gamma.locator(
    "xpath=ancestor::*[contains(@class, 'slider-root')][1]",
  );
  const originalGamma = Number(await gamma.getAttribute("aria-valuenow"));
  const gammaThumbBounds = await gamma.boundingBox();
  const gammaBounds = await gammaRoot.boundingBox();
  expect(gammaThumbBounds).not.toBeNull();
  expect(gammaBounds).not.toBeNull();
  if (gammaThumbBounds === null || gammaBounds === null) return;
  await page.mouse.move(
    gammaThumbBounds.x + gammaThumbBounds.width / 2,
    gammaThumbBounds.y + gammaThumbBounds.height / 2,
  );
  await page.mouse.down();
  await page.mouse.move(
    gammaBounds.x + gammaBounds.width * 0.8,
    gammaBounds.y + gammaBounds.height / 2,
  );
  await page.mouse.up();
  await expect(gamma).not.toHaveAttribute(
    "aria-valuenow",
    String(originalGamma),
  );

  await page.getByRole("button", { name: "Close reader settings" }).click();
  await page.keyboard.press("ArrowRight");
  await expect(page.locator(".paged-canvas")).toHaveClass(
    /motion-(?:forward|backward)/,
  );
});

test("touch layout remains usable at iPhone dimensions", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openCleanPreview(page);
  await expect(page.getByRole("navigation", { name: "Primary" })).toBeVisible();
  await expectNoAccessibilityViolations(page, "Touch library");
  await evidence(page, "library-mobile");

  await page.getByRole("button", { name: "Continue reading" }).click();
  await expect(page.getByLabel("Reading After the Last Train")).toBeVisible();
  await expect(page.getByRole("button", { name: "Next page" })).toBeVisible();
  await evidence(page, "reader-mobile");
});

test("dark library and reader preserve accessible contrast", async ({ page }) => {
  await openCleanPreview(page);
  await page.evaluate(() => localStorage.setItem("koma.theme", "dark"));
  await page.reload();
  await expect(
    page.getByRole("button", { name: "Continue reading" }),
  ).toBeVisible();
  await expectNoAccessibilityViolations(page, "Dark library");

  await page.getByRole("button", { name: "Continue reading" }).click();
  await expect(page.getByLabel("Reading After the Last Train")).toBeVisible();
  await page.keyboard.press("s");
  await expect(
    page.getByRole("heading", { name: "Reader settings" }),
  ).toBeVisible();
  await expectNoAccessibilityViolations(page, "Dark reader settings");
});
