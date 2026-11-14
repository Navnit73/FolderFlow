import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import type {
  ActivityFilter,
  ActivityPage as ActivityPageData,
  OperationView,
  PageRequest,
  RevealTarget,
  ReviewDecision,
  ReviewItemView,
  ReviewResolveReport,
  UndoReport,
  UndoTarget,
} from "@/services/activity";
import { useActivity } from "@/stores/activity";
import { ActivityPage } from "./ActivityPage";

const mockService = vi.hoisted(() => ({
  list: vi.fn<(filter: ActivityFilter, page: PageRequest) => Promise<ActivityPageData>>(),
  undo: vi.fn<(target: UndoTarget) => Promise<UndoReport>>(),
  retry: vi.fn<(opId: string) => Promise<OperationView>>(),
  exportCsv: vi.fn<(filter: ActivityFilter) => Promise<string>>(),
  clear: vi.fn<(beforeMs?: number | null) => Promise<number>>(),
  revealInFinder: vi.fn<(opId: string, which: RevealTarget) => Promise<boolean>>(),
  reviewList: vi.fn<() => Promise<ReviewItemView[]>>(),
  reviewResolve: vi.fn<(id: string, decision: ReviewDecision) => Promise<ReviewResolveReport>>(),
}));

vi.mock("@/services/activity", () => ({
  activityService: mockService,
}));

function makeOp(overrides: Partial<OperationView> = {}): OperationView {
  return {
    id: "op-1",
    batchId: "batch-1",
    operationType: "move",
    sourcePath: "/Users/alice/Downloads/invoice.pdf",
    destinationPath: "/Users/alice/Documents/Invoices/invoice.pdf",
    originalName: "invoice.pdf",
    newName: "invoice.pdf",
    size: 24576,
    contentHash: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90",
    crossVolume: false,
    ruleId: "rule-1",
    ruleName: "Sort Invoices",
    status: "COMPLETED",
    reversible: true,
    reversedBy: null,
    errorCode: null,
    errorMessage: null,
    startedAt: 1_700_000_000_000,
    completedAt: 1_700_000_000_120,
    ...overrides,
  };
}

function makeReview(overrides: Partial<ReviewItemView> = {}): ReviewItemView {
  return {
    id: "rev-1",
    path: "/Users/alice/Downloads/tax-2026.pdf",
    reason: "destination_exists",
    ruleId: "rule-1",
    proposed: {
      operationType: "move",
      sourcePath: "/Users/alice/Downloads/tax-2026.pdf",
      destinationPath: "/Users/alice/Documents/Taxes/tax-2026.pdf",
      originalName: "tax-2026.pdf",
      newName: "tax-2026.pdf",
    },
    createdAt: 1_700_000_010_000,
    resolvedAt: null,
    ...overrides,
  };
}

function renderActivity() {
  return render(
    <MemoryRouter initialEntries={["/activity"]}>
      <Routes>
        <Route path="/activity" element={<ActivityPage />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("Activity screen (P4-T10)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useActivity.getState().reset();
    mockService.list.mockResolvedValue({
      items: [
        makeOp({ id: "op-1", originalName: "invoice.pdf", newName: "invoice.pdf" }),
        makeOp({
          id: "op-2",
          operationType: "copy",
          originalName: "photo.png",
          newName: "photo.png",
          sourcePath: "/Users/alice/Downloads/photo.png",
          destinationPath: "/Users/alice/Pictures/photo.png",
          ruleId: "rule-2",
          ruleName: "Sort Photos",
          status: "FAILED",
          errorCode: "enospc",
          errorMessage: "No space left on device",
        }),
      ],
      total: 2,
      limit: 50,
      offset: 0,
    });
    mockService.reviewList.mockResolvedValue([]);
    mockService.undo.mockResolvedValue({
      undone: [],
      conflicts: [],
      failed: [],
      prunedDirs: 0,
    });
    mockService.retry.mockResolvedValue(
      makeOp({ id: "op-2-retry", originalName: "photo.png", status: "COMPLETED" }),
    );
    mockService.exportCsv.mockResolvedValue("id,started_at\nop-1,1700000000000\n");
    mockService.clear.mockResolvedValue(2);
    mockService.revealInFinder.mockResolvedValue(true);
    mockService.reviewResolve.mockResolvedValue({
      reviewId: "rev-1",
      decision: "keep_both",
      operation: makeOp({ id: "op-resolved" }),
    });
  });

  it("UI-ACT-01: renders operation history rows, status badges, details, and filters via activityService.list", async () => {
    const user = userEvent.setup();
    renderActivity();

    expect(await screen.findByText("invoice.pdf")).toBeInTheDocument();
    expect(screen.getByText("Sort Invoices")).toBeInTheDocument();
    expect(screen.getByText("photo.png")).toBeInTheDocument();
    expect(screen.getByText(/No space left on device/i)).toBeInTheDocument();

    // Filter by status = FAILED
    const statusSelect = screen.getByLabelText("Filter by status");
    await user.selectOptions(statusSelect, "FAILED");
    await waitFor(() => {
      expect(mockService.list).toHaveBeenLastCalledWith(
        expect.objectContaining({ status: "FAILED" }),
        expect.objectContaining({ offset: 0 }),
      );
    });

    // Filter by search query
    const searchInput = screen.getByLabelText("Search activity");
    await user.type(searchInput, "invoice");
    await waitFor(() => {
      expect(mockService.list).toHaveBeenLastCalledWith(
        expect.objectContaining({ search: "invoice" }),
        expect.objectContaining({ offset: 0 }),
      );
    });
  });

  it("UI-ACT-02: Undo Last and per-row Undo invoke activity_undo and surface occupied-path conflicts with auto-rename resolution", async () => {
    const user = userEvent.setup();
    mockService.undo
      .mockResolvedValueOnce({
        undone: [],
        conflicts: [
          {
            opId: "op-1",
            originalPath: "/Users/alice/Downloads/invoice.pdf",
            currentPath: "/Users/alice/Documents/Invoices/invoice.pdf",
            reason: "Original location is now occupied by another file.",
          },
        ],
        failed: [],
        prunedDirs: 0,
      })
      .mockResolvedValueOnce({
        undone: [makeOp({ id: "op-undo-1", operationType: "undo_move" })],
        conflicts: [],
        failed: [],
        prunedDirs: 0,
      });

    renderActivity();
    await screen.findByText("invoice.pdf");

    await user.click(screen.getByRole("button", { name: "Undo Last" }));
    await waitFor(() => {
      expect(mockService.undo).toHaveBeenCalledWith({
        kind: "last",
        auto_rename_conflicts: false,
      });
    });

    // Occupied-path conflict banner appears and offers "Restore as Copy (Auto-Rename)"
    expect(
      await screen.findByText(/Original location is now occupied by another file/i),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: /Restore with Auto-Rename/i }),
    );
    await waitFor(() => {
      expect(mockService.undo).toHaveBeenCalledWith({
        kind: "ops",
        op_ids: ["op-1"],
        auto_rename_conflicts: true,
      });
    });
  });

  it("UI-ACT-03: multi-select Undo Selected, Undo Batch, and Retry Failed operation", async () => {
    const user = userEvent.setup();
    renderActivity();
    await screen.findByText("invoice.pdf");

    // Select row op-1 and click Undo Selected
    await user.click(screen.getByLabelText("Select operation op-1"));
    await user.click(screen.getByRole("button", { name: /Undo Selected/i }));
    await waitFor(() => {
      expect(mockService.undo).toHaveBeenCalledWith({
        kind: "ops",
        op_ids: ["op-1"],
        auto_rename_conflicts: false,
      });
    });

    // Undo Batch button on row with batchId
    await user.click(screen.getByRole("button", { name: "Undo batch for op-1" }));
    await waitFor(() => {
      expect(mockService.undo).toHaveBeenCalledWith({
        kind: "batch",
        batch_id: "batch-1",
        restore_structure: true,
        auto_rename_conflicts: false,
      });
    });

    // Retry Failed on op-2
    await user.click(screen.getByRole("button", { name: "Retry operation op-2" }));
    await waitFor(() => {
      expect(mockService.retry).toHaveBeenCalledWith("op-2");
    });
  });

  it("UI-ACT-04: Export CSV, Clear History confirmation dialog, and Reveal in Finder", async () => {
    const user = userEvent.setup();
    renderActivity();
    await screen.findByText("invoice.pdf");

    // Reveal destination in Finder
    await user.click(
      screen.getByRole("button", { name: "Reveal destination in Finder for op-1" }),
    );
    await waitFor(() => {
      expect(mockService.revealInFinder).toHaveBeenCalledWith("op-1", "destination");
    });

    // Export CSV
    await user.click(screen.getByRole("button", { name: "Export CSV" }));
    await waitFor(() => {
      expect(mockService.exportCsv).toHaveBeenCalled();
    });
    expect(await screen.findByText(/CSV exported/i)).toBeInTheDocument();

    // Clear History opens confirmation dialog first
    await user.click(screen.getByRole("button", { name: "Clear History" }));
    expect(
      await screen.findByRole("dialog", { name: /Clear Activity History/i }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Confirm Clear History" }));
    await waitFor(() => {
      expect(mockService.clear).toHaveBeenCalledWith(null);
    });
  });

  it("UI-ACT-05: renders pending Review Queue conflicts and resolves via Keep Both or Skip", async () => {
    const user = userEvent.setup();
    mockService.reviewList.mockResolvedValue([makeReview()]);

    renderActivity();
    expect(await screen.findByText("tax-2026.pdf")).toBeInTheDocument();
    expect(screen.getByText(/Needs Review/i)).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Keep Both for review rev-1" }),
    );
    await waitFor(() => {
      expect(mockService.reviewResolve).toHaveBeenCalledWith("rev-1", "keep_both");
    });
  });
});
