import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { routes } from "@/app/routes";
import { useDashboard } from "@/stores/dashboard";
import { useFolders } from "@/stores/folders";
import { useSettings } from "@/stores/settings";
import {
  commands,
  dashboardStatsFixture,
  emitFolderActivity,
  folderFixture,
  resetBindingMocks,
  runtimeFixture,
} from "@/test/mockBindings";

vi.mock("@/bindings", () => import("@/test/mockBindings"));

const ok = <T,>(data: T) => ({ status: "ok" as const, data });
const err = (message: string, code = "database") => ({
  status: "error" as const,
  error: { code, message, hint: null, field: null },
});

async function renderOverview() {
  useSettings.setState({ settings: null, status: "idle", saving: false, loadError: null, saveError: null });
  useDashboard.getState().reset();
  useFolders.getState().reset();
  await useSettings.getState().load();
  const router = createMemoryRouter(routes, { initialEntries: ["/overview"] });
  render(<RouterProvider router={router} />);
  return router;
}

beforeEach(() => {
  resetBindingMocks();
});

describe("OverviewPage (UI-DASH-01..02)", () => {
  it("UI-DASH-01: renders SQLite zero-state metrics with no hardcoded placeholder figures and retries on error", async () => {
    commands.dashboardStats
      .mockResolvedValueOnce(err("SQLite stats unavailable"))
      .mockResolvedValueOnce(
        ok(
          dashboardStatsFixture({
            dailyActivity: [
              { date: "2026-11-08", count: 0 },
              { date: "2026-11-09", count: 0 },
              { date: "2026-11-10", count: 0 },
              { date: "2026-11-11", count: 0 },
              { date: "2026-11-12", count: 0 },
              { date: "2026-11-13", count: 0 },
              { date: "2026-11-14", count: 0 },
            ],
          }),
        ),
      );

    await renderOverview();
    expect(await screen.findByRole("alert")).toHaveTextContent("SQLite stats unavailable");

    await userEvent.click(screen.getByRole("button", { name: "Try again" }));

    expect(await screen.findByText("No active folders")).toBeInTheDocument();
    expect(screen.getByText("0 of 0 rules active · 0 of 0 folders enabled")).toBeInTheDocument();
    expect(screen.getByText(/No tracked files yet/i)).toBeInTheDocument();
    expect(screen.getByText(/No rules created yet/i)).toBeInTheDocument();
    expect(screen.getByText("No file operations recorded yet.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Pause all/i })).toBeDisabled();
  });

  it("UI-DASH-02: renders populated metrics, pauses monitoring, navigates via quick actions, and refreshes on folder activity", async () => {
    commands.foldersList.mockResolvedValue(ok([folderFixture({ id: "f1", name: "Downloads" })]));
    commands.monitoringStatus.mockResolvedValue(
      ok({ paused: false, folders: [runtimeFixture({ folderId: "f1" })] }),
    );
    commands.dashboardStats.mockResolvedValue(
      ok(
        dashboardStatsFixture({
          filesOrganizedTotal: 42,
          filesOrganizedToday: 5,
          filesOrganizedThisWeek: 18,
          totalBytesOrganized: 12_500_000,
          activeFoldersCount: 1,
          totalFoldersCount: 2,
          activeRulesCount: 3,
          totalRulesCount: 4,
          pendingReviewCount: 2,
          failedOperationsCount: 1,
          totalTrackedFiles: 120,
          totalTrackedBytes: 85_000_000,
          duplicateGroupsCount: 3,
          duplicateWastedBytes: 4_000_000,
          categoryBreakdown: [
            {
              categoryId: "documents",
              categoryName: "Documents",
              colorToken: "cat-documents",
              fileCount: 70,
              totalBytes: 50_000_000,
            },
          ],
          topRules: [
            {
              ruleId: "r-inv",
              ruleName: "Archive Invoices",
              enabled: true,
              matchedOps: 19,
              failedOps: 1,
              bytesOrganized: 8_200_000,
              lastRunAtMs: Date.now() - 60_000,
            },
          ],
          dailyActivity: [{ date: "2026-11-14", count: 5 }],
          recentOperations: [
            {
              id: "op-1",
              batchId: "b-1",
              operationType: "move",
              status: "COMPLETED",
              originalName: "receipt.pdf",
              newName: "2026-11_receipt.pdf",
              sourcePath: "/Users/test/Downloads/receipt.pdf",
              destinationPath: "/Users/test/Documents/2026-11_receipt.pdf",
              size: 25_000,
              contentHash: null,
              crossVolume: false,
              ruleId: "r-inv",
              ruleName: "Archive Invoices",
              reversible: true,
              reversedBy: null,
              errorCode: null,
              errorMessage: null,
              startedAt: Date.now() - 10_000,
              completedAt: Date.now() - 5_000,
            },
          ],
        }),
      ),
    );

    const router = await renderOverview();

    expect(await screen.findByText("Watching 1 folder")).toBeInTheDocument();
    expect(screen.getByText("18 this week · 42 all time")).toBeInTheDocument();
    expect(screen.getByText("3 active duplicate groups")).toBeInTheDocument();
    expect(screen.getByText("2 pending review · 1 failed")).toBeInTheDocument();
    expect(screen.getByText("Documents")).toBeInTheDocument();
    expect(screen.getByText("Archive Invoices")).toBeInTheDocument();
    expect(screen.getByText("2026-11_receipt.pdf")).toBeInTheDocument();

    // Pause all triggers monitoringSetPaused(true)
    await userEvent.click(screen.getByRole("button", { name: /Pause all/i }));
    await waitFor(() => expect(commands.monitoringSetPaused).toHaveBeenCalledWith(true));
    expect(await screen.findByText("All monitoring paused")).toBeInTheDocument();

    // Live folder activity event triggers stats refresh
    const callsBeforeEvent = commands.dashboardStats.mock.calls.length;
    await act(async () => {
      emitFolderActivity({
        folderId: "f1",
        kind: "stable",
      });
    });
    await waitFor(() =>
      expect(commands.dashboardStats.mock.calls.length).toBeGreaterThan(callsBeforeEvent),
    );

    // Quick action navigates cleanly
    await userEvent.click(screen.getByRole("button", { name: /Find duplicates/i }));
    expect(router.state.location.pathname).toBe("/duplicates");
  });
});
