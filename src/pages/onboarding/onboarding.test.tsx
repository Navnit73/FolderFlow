import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { App } from "@/App";
import { SettingsPage } from "@/pages/settings/SettingsPage";
import { useOnboarding } from "@/stores/onboarding";
import { useSettings } from "@/stores/settings";
import {
  commands,
  folderFixture,
  onboardingStateFixture,
  planItemFixture,
  planSummaryFixture,
  presetFixture,
  resetBindingMocks,
} from "@/test/mockBindings";
import { OnboardingPage } from "./OnboardingPage";

vi.mock("@/bindings", () => import("@/test/mockBindings"));

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

function renderOnboardingRoute(initialPath = "/onboarding/1") {
  return render(
    <MemoryRouter initialEntries={[initialPath]}>
      <Routes>
        <Route path="/onboarding" element={<OnboardingPage />} />
        <Route path="/onboarding/:step" element={<OnboardingPage />} />
        <Route path="/folders" element={<div>Folders Destination</div>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("OnboardingPage (UI-ONB-01..04)", () => {
  beforeEach(() => {
    resetBindingMocks();
    useOnboarding.getState().resetStore();
    useSettings.setState({ status: "idle", settings: null, loadError: null, saveError: null });
    commands.onboardingState.mockResolvedValue(
      ok(onboardingStateFixture({ currentStep: 1, completed: false, automationConfirmed: false, completedAt: null })),
    );
    commands.foldersAddKnown.mockImplementation(async (kind: string) =>
      ok(folderFixture({ id: `f-${kind}`, name: kind, path: `/Users/test/${kind}` })),
    );
    commands.rulesPresets.mockResolvedValue(
      ok([
        presetFixture({ id: "pdfs", name: "Organize PDFs" }),
        presetFixture({ id: "screenshots", name: "Archive Screenshots" }),
      ]),
    );
  });

  it("UI-ONB-01 & UI-ONB-04: walks through all 6 steps and requires explicit confirmation to enable automation", async () => {
    commands.onboardingScanPreview.mockResolvedValue(
      ok({
        foldersScanned: 1,
        totalFiles: 12,
        totalFolders: 1,
        totalBytes: 4096,
        byCategory: [{ category: "documents", count: 8, bytes: 3000 }],
        planSummary: planSummaryFixture({ filesEvaluated: 12, filesMatched: 4 }),
        sampleItems: [planItemFixture({ id: "i1", originalName: "tax_return.pdf" })],
      }),
    );

    renderOnboardingRoute("/onboarding/1");

    // Step 1: Welcome
    expect(await screen.findByRole("heading", { name: /Welcome to FolderFlow/i })).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /Get started/i }));

    // Step 2: Select Folders
    expect(await screen.findByRole("heading", { name: /Choose folders to watch/i })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Add Downloads/i }));
    await waitFor(() => {
      expect(commands.foldersAddKnown).toHaveBeenCalledWith("downloads", false);
    });
    await user.click(screen.getByRole("button", { name: /Continue/i }));

    // Step 3: Organization Preference
    expect(await screen.findByRole("heading", { name: /How should FolderFlow organize/i })).toBeInTheDocument();
    await user.click(screen.getByRole("radio", { name: /By date/i }));
    await user.click(screen.getByRole("button", { name: /Continue/i }));

    // Step 4: Starter Rules
    expect(await screen.findByRole("heading", { name: /Choose starter rules/i })).toBeInTheDocument();
    expect(screen.getByText("Organize PDFs")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Scan & preview/i }));

    // Step 5: Initial Scan & Insights
    await waitFor(() => {
      expect(commands.onboardingScanPreview).toHaveBeenCalled();
    });
    expect(await screen.findByRole("heading", { name: /What FolderFlow found/i })).toBeInTheDocument();
    expect(screen.getByText("12")).toBeInTheDocument();
    expect(screen.getByText(/Zero files were modified/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Inspect preview/i }));

    // Step 6: First Preview & Explicit Confirmation (default unchecked = safe)
    expect(await screen.findByRole("heading", { name: /Review your first preview/i })).toBeInTheDocument();
    expect(screen.getByText("tax_return.pdf")).toBeInTheDocument();
    const confirmCheckbox = screen.getByRole("checkbox", {
      name: /Enable selected starter rules/i,
    });
    expect(confirmCheckbox).not.toBeChecked();

    await user.click(confirmCheckbox);
    expect(confirmCheckbox).toBeChecked();

    await user.click(screen.getByRole("button", { name: /Finish setup/i }));
    await waitFor(() => {
      expect(commands.onboardingComplete).toHaveBeenCalledWith(
        expect.arrayContaining(["pdfs"]),
        true,
      );
    });
    expect(await screen.findByText("Folders Destination")).toBeInTheDocument();
  });

  it("UI-ONB-02: explains macOS TCC permission denial and opens Privacy & Security settings", async () => {
    commands.foldersList.mockResolvedValue(ok([folderFixture({ id: "f1", name: "Downloads" })]));
    commands.foldersAccess.mockResolvedValue(ok("denied"));

    renderOnboardingRoute("/onboarding/2");

    expect(
      await screen.findByText(/macOS denied access to Downloads/i),
    ).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /Open Privacy & Security Settings/i }));
    await waitFor(() => {
      expect(commands.openPrivacySettings).toHaveBeenCalledTimes(1);
    });
  });

  it("UI-ONB-03: resumes at saved onboarding step and lets Settings re-run onboarding", async () => {
    window.location.hash = "";
    commands.onboardingState.mockResolvedValue(
      ok(onboardingStateFixture({ currentStep: 3, completed: false, automationConfirmed: false })),
    );

    render(<App />);

    expect(
      await screen.findByRole("heading", { name: /How should FolderFlow organize/i }),
    ).toBeInTheDocument();

    // Now test re-running from Settings -> General
    resetBindingMocks();
    useOnboarding.getState().resetStore();
    render(
      <MemoryRouter initialEntries={["/settings/general"]}>
        <Routes>
          <Route path="/settings/:section" element={<SettingsPage />} />
          <Route path="/onboarding/:step" element={<OnboardingPage />} />
        </Routes>
      </MemoryRouter>,
    );

    const rerunBtn = await screen.findByRole("button", { name: /Re-run setup wizard/i });
    const user = userEvent.setup();
    await user.click(rerunBtn);

    await waitFor(() => {
      expect(commands.onboardingReset).toHaveBeenCalledTimes(1);
    });
    expect(
      await screen.findByRole("heading", { name: /Welcome to FolderFlow/i }),
    ).toBeInTheDocument();
  });
});

