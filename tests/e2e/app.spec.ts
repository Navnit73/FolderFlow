import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { installTauriMock } from "./helpers/tauriMock";

const PRIMARY_ROUTES = [
  "/#/overview",
  "/#/folders",
  "/#/folders/f1",
  "/#/rules",
  "/#/rules/new",
  "/#/preview",
  "/#/activity",
  "/#/duplicates",
  "/#/scheduler",
  "/#/categories",
  "/#/settings/general",
  "/#/settings/monitoring",
  "/#/settings/organization",
  "/#/settings/performance",
  "/#/settings/notifications",
  "/#/settings/appearance",
  "/#/settings/privacy",
  "/#/settings/backup",
  "/#/settings/about",
  "/#/license",
];

async function assertNoAxeViolations(page: Page, contextLabel: string): Promise<void> {
  // Let theme/nav CSS color transitions settle before computing element contrast.
  await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished)));
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
    .analyze();
  expect(
    results.violations,
    `Expected 0 WCAG A/AA violations on ${contextLabel}, found: ${JSON.stringify(
      results.violations.map((v) => ({
        id: v.id,
        impact: v.impact,
        description: v.description,
        nodes: v.nodes.map((n) => n.html),
      })),
      null,
      2,
    )}`,
  ).toEqual([]);
}

test.describe("FolderFlow E2E workflows (E2E-01..06 & P8-T06)", () => {
  test("E2E-01: completes 6-step onboarding flow without mutating files", async ({ page }) => {
    await installTauriMock(page, { onboardingCompleted: false });
    await page.goto("/");

    await expect(page.getByRole("heading", { name: "Welcome to FolderFlow" })).toBeVisible();
    await assertNoAxeViolations(page, "onboarding/1");
    await page.getByRole("button", { name: "Get started" }).click();

    await expect(page.getByRole("heading", { name: "Choose folders to watch" })).toBeVisible();
    await page.getByRole("button", { name: "Add Downloads" }).click();
    await expect(page.getByText("Access granted")).toBeVisible();
    await page.getByRole("button", { name: "Continue" }).click();

    await expect(
      page.getByRole("heading", { name: "How should FolderFlow organize your files?" }),
    ).toBeVisible();
    await page.getByRole("radio", { name: /By category/i }).click();
    await page.getByRole("button", { name: "Continue" }).click();

    await expect(page.getByRole("heading", { name: "Choose starter rules" })).toBeVisible();
    await page.getByRole("button", { name: /Scan & preview/i }).click();

    await expect(page.getByRole("heading", { name: "What FolderFlow found" })).toBeVisible();
    await expect(page.getByText("12")).toBeVisible();
    await page.getByRole("button", { name: "Inspect preview" }).click();

    await expect(page.getByRole("heading", { name: "Review your first preview" })).toBeVisible();
    await expect(page.getByText("2 proposed file changes")).toBeVisible();
    await page.getByRole("button", { name: "Finish setup" }).click();

    await expect(page.getByRole("heading", { name: "Folders", exact: true })).toBeVisible();
    await expect(page.getByRole("link", { name: "Downloads" })).toBeVisible();
  });

  test("E2E-02 & E2E-03: manages monitored folders and builds/validates/edits rules", async ({
    page,
  }) => {
    await installTauriMock(page, { onboardingCompleted: true });
    await page.goto("/#/folders");

    await expect(page.getByRole("link", { name: "Downloads" })).toBeVisible();
    await page.getByRole("button", { name: "Pause Downloads" }).click();
    await expect(page.getByRole("button", { name: "Resume Downloads" })).toBeVisible();
    await page.getByRole("button", { name: "Resume Downloads" }).click();

    await page.getByRole("link", { name: "Downloads" }).click();
    await expect(page.getByRole("heading", { name: "Downloads" })).toBeVisible();
    await page.getByLabel("Wait before processing (seconds)").fill("4");
    await page.getByRole("button", { name: "Save changes" }).click();
    await expect(page.getByRole("status").filter({ hasText: "Saved" })).toBeVisible();
    await page.getByRole("button", { name: "Folders" }).click();

    await page.getByRole("link", { name: "Rules" }).click();
    await expect(page.getByRole("link", { name: "Sort Invoices" })).toBeVisible();
    await page.getByRole("button", { name: "New rule" }).click();

    await expect(page.getByRole("heading", { name: "New Rule" })).toBeVisible();
    const ruleNameInput = page.getByLabel("Rule name");
    await ruleNameInput.fill("");
    await expect(page.getByRole("alert")).toContainText("Rule name cannot be empty.");

    await ruleNameInput.fill("Organize Receipts E2E");
    await page.getByRole("button", { name: "Test rule on folder" }).click();
    await expect(page.getByText("Matched 1 of 4 files in Downloads")).toBeVisible();

    await page.getByRole("button", { name: "Save rule" }).click();
    await expect(page.getByRole("link", { name: "Organize Receipts E2E" })).toBeVisible();

    await page.getByRole("link", { name: "Organize Receipts E2E" }).click();
    await expect(page.getByRole("heading", { name: /Edit Rule: Organize Receipts E2E/ })).toBeVisible();
    await page.getByLabel("Rule name").fill("Organize Receipts Updated");
    await page.getByRole("button", { name: "Save rule" }).click();
    await expect(page.getByRole("link", { name: "Organize Receipts Updated" })).toBeVisible();
  });

  test("E2E-04 & E2E-05: executes selected preview batch via shortcut, resolves review, exports CSV, and undoes", async ({
    page,
  }) => {
    await installTauriMock(page, { onboardingCompleted: true });
    await page.goto("/#/preview");

    await expect(page.getByText("Invoice_Jan.pdf", { exact: true })).toBeVisible();
    await page.getByLabel("Select Screenshot 2025-01-01.png").uncheck();
    await expect(page.getByRole("button", { name: "Execute selected (1)" })).toBeEnabled();

    await page.keyboard.press("Meta+Enter");
    await expect(page.getByRole("dialog")).toContainText("Apply planned changes?");
    await page.getByRole("button", { name: "Apply changes" }).click();

    await expect(page.getByRole("status")).toContainText("Executed 1 change");
    await page.getByRole("link", { name: /Open Activity & Undo/ }).click();

    await expect(page.getByRole("heading", { name: "Activity" })).toBeVisible();
    await expect(page.getByRole("heading", { name: /Needs Review \(1\)/ })).toBeVisible();
    await page.getByRole("button", { name: "Keep Both for review rev-1" }).click();
    await expect(page.getByRole("heading", { name: /Needs Review/ })).toBeHidden();

    await page.getByRole("button", { name: "Export CSV" }).click();
    await expect(page.getByRole("status")).toContainText("CSV exported");

    await page.getByRole("button", { name: "Undo Last" }).click();
    await expect(page.getByText("undo_move").first()).toBeVisible();
  });

  test("E2E-06: toggles Settings appearance/density, handles shortcuts, and activates offline license", async ({
    page,
  }) => {
    await installTauriMock(page, { onboardingCompleted: true });
    await page.goto("/#/overview");

    await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
    await page.keyboard.press("Meta+Comma");
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();

    await page.getByRole("radio", { name: "Dark" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

    await page.getByRole("radio", { name: "Compact" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-density", "compact");

    await page.getByRole("radio", { name: "Light" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");

    await page.getByRole("link", { name: "Unlicensed" }).click();
    await expect(page.getByRole("heading", { name: "License", exact: true })).toBeVisible();
    await page
      .getByLabel("License key or offline signed key")
      .fill("FF1.eyJlZGl0aW9uIjoibGlmZXRpbWUifQ.valid-signature");
    await page.getByRole("button", { name: "Activate License" }).click();

    await expect(page.getByRole("heading", { name: "FolderFlow is licensed" })).toBeVisible();
    await expect(page.getByRole("link", { name: "Licensed ✓" })).toBeVisible();
  });

  test("P8-T06: passes WCAG 2.1 AA axe-core and keyboard/landmark accessibility across all routes in Light and Dark modes", async ({
    page,
  }) => {
    await installTauriMock(page, { onboardingCompleted: true });

    for (const theme of ["light", "dark"] as const) {
      await page.goto("/#/settings/appearance");
      await page.getByRole("radio", { name: theme === "light" ? "Light" : "Dark" }).click();
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);

      for (const route of PRIMARY_ROUTES) {
        await page.goto(route);
        await expect(page.locator("main")).toBeVisible();
        await expect(page.locator("nav[aria-label='Main']")).toBeVisible();
        await assertNoAxeViolations(page, `${theme} mode ${route}`);
      }
    }

    await page.goto("/#/overview");
    await page.keyboard.press("Tab");
    const activeTag = await page.evaluate(() => document.activeElement?.tagName ?? "");
    expect(["A", "BUTTON", "INPUT", "SELECT"]).toContain(activeTag);
  });
});

