import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import {
  commands,
  folderFixture,
  planItemFixture,
  planSummaryFixture,
  resetBindingMocks,
  ruleFixture,
} from "@/test/mockBindings";
import { usePreview } from "@/stores/preview";
import { PreviewPage } from "./PreviewPage";

vi.mock("@/bindings", () => import("@/test/mockBindings"));

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

function renderPreview() {
  return render(
    <MemoryRouter>
      <PreviewPage />
    </MemoryRouter>,
  );
}

describe("PreviewPage (UI-PREV-01..04)", () => {
  beforeEach(() => {
    resetBindingMocks();
    usePreview.getState().reset();
    commands.foldersList.mockResolvedValue(ok([folderFixture()]));
    commands.rulesList.mockResolvedValue(ok([ruleFixture()]));
  });

  it("UI-PREV-01: generates dry-run plan, shows summary breakdown, and supports filters", async () => {
    const summary = planSummaryFixture({
      filesEvaluated: 14,
      filesMatched: 2,
      filesWithConflicts: 0,
      totalBytesAffected: 3500,
      crossVolumeBytes: 1500,
      byCategory: [
        { category: "documents", count: 1, bytes: 2000 },
        { category: "images", count: 1, bytes: 1500 },
      ],
      byRule: [
        { ruleId: "r1", ruleName: "Organize Invoices", count: 2 },
      ],
    });
    const items = [
      planItemFixture({
        id: "i1",
        originalName: "invoice_april.pdf",
        proposedName: "invoice_april.pdf",
        proposedDestination: "/Users/test/Documents/Invoices/invoice_april.pdf",
        category: "documents",
      }),
      planItemFixture({
        id: "i2",
        originalName: "receipt.png",
        proposedName: "receipt.png",
        proposedDestination: "/Users/test/Pictures/Receipts/receipt.png",
        category: "images",
      }),
    ];
    commands.planCreate.mockResolvedValue(ok(summary));
    commands.planItems.mockResolvedValue(ok({ items, total: 2, limit: 100, offset: 0 }));

    renderPreview();

    expect(await screen.findByText("invoice_april.pdf")).toBeInTheDocument();
    expect(screen.getByText("receipt.png")).toBeInTheDocument();
    expect(screen.getByText("14")).toBeInTheDocument();
    expect(screen.getByText("3.5 KB")).toBeInTheDocument();
    expect(commands.planCreate).toHaveBeenCalledTimes(1);

    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Search planned files"), "invoice");
    await waitFor(() => {
      expect(commands.planItems).toHaveBeenLastCalledWith(
        "plan-1",
        expect.objectContaining({ search: "invoice" }),
        100,
        0,
      );
    });
  });

  it("UI-PREV-02: deselects individual items before executing", async () => {
    const items = [
      planItemFixture({ id: "i1", originalName: "alpha.pdf" }),
      planItemFixture({ id: "i2", originalName: "beta.pdf" }),
    ];
    commands.planItems.mockResolvedValue(ok({ items, total: 2, limit: 100, offset: 0 }));
    commands.planExecute.mockResolvedValue(
      ok({
        planId: "plan-1",
        batchId: "b1",
        completed: [],
        queuedForReview: 0,
        skippedStale: [],
        failed: [],
      }),
    );

    renderPreview();
    expect(await screen.findByText("alpha.pdf")).toBeInTheDocument();

    const user = userEvent.setup();
    const betaCheckbox = screen.getByRole("checkbox", { name: "Select beta.pdf" });
    expect(betaCheckbox).toBeChecked();
    await user.click(betaCheckbox);
    expect(betaCheckbox).not.toBeChecked();

    await user.click(screen.getByRole("button", { name: /Execute selected/i }));
    const confirmBtn = await screen.findByRole("button", { name: "Apply changes" });
    await user.click(confirmBtn);

    await waitFor(() => {
      expect(commands.planExecute).toHaveBeenCalledWith("plan-1", ["i1"], "auto_rename");
    });
  });

  it("UI-PREV-03: highlights conflicts and disables blocked items from selection", async () => {
    const items = [
      planItemFixture({
        id: "i-ok",
        originalName: "clean.pdf",
        status: "ready",
      }),
      planItemFixture({
        id: "i-conflict",
        originalName: "dup.pdf",
        status: "conflict",
        conflicts: [
          {
            kind: { kind: "destination_exists_on_disk" },
            message: "Destination file already exists on disk.",
          },
        ],
      }),
      planItemFixture({
        id: "i-blocked",
        originalName: "unauthorized.pdf",
        status: "blocked",
        conflicts: [
          {
            kind: { kind: "destination_unauthorized" },
            message: "Destination is outside approved roots.",
          },
        ],
      }),
    ];
    commands.planCreate.mockResolvedValue(ok(planSummaryFixture({ filesMatched: 3, filesWithConflicts: 2 })));
    commands.planItems.mockResolvedValue(ok({ items, total: 3, limit: 100, offset: 0 }));

    renderPreview();

    expect(await screen.findByText("Destination file already exists on disk.")).toBeInTheDocument();
    expect(screen.getByText("Destination is outside approved roots.")).toBeInTheDocument();

    const blockedBox = screen.getByRole("checkbox", { name: "Select unauthorized.pdf" });
    expect(blockedBox).toBeDisabled();
    expect(blockedBox).not.toBeChecked();

    expect(screen.getByRole("button", { name: /Execute selected \(2\)/i })).toBeInTheDocument();
  });

  it("UI-PREV-04: confirms and executes via Cmd+Enter shortcut and shows result report", async () => {
    const items = [planItemFixture({ id: "i1", originalName: "tax_2026.pdf" })];
    commands.planItems.mockResolvedValue(ok({ items, total: 1, limit: 100, offset: 0 }));
    commands.planExecute.mockResolvedValue(
      ok({
        planId: "plan-1",
        batchId: "batch-99",
        completed: [
          {
            id: "op1",
            batchId: "batch-99",
            operationType: "move",
            sourcePath: "/Users/test/Downloads/tax_2026.pdf",
            destinationPath: "/Users/test/Documents/Invoices/tax_2026.pdf",
            originalName: "tax_2026.pdf",
            newName: "tax_2026.pdf",
            size: 1024,
            contentHash: null,
            crossVolume: false,
            ruleId: "r1",
            ruleName: "Organize Invoices",
            status: "COMPLETED",
            reversible: true,
            reversedBy: null,
            errorCode: null,
            errorMessage: null,
            startedAt: Date.now(),
            completedAt: Date.now(),
          },
        ],
        queuedForReview: 0,
        skippedStale: [],
        failed: [],
      }),
    );

    renderPreview();
    expect(await screen.findByText("tax_2026.pdf")).toBeInTheDocument();

    const user = userEvent.setup();
    await user.keyboard("{Meta>}{Enter}{/Meta}");

    const dialogConfirm = await screen.findByRole("button", { name: "Apply changes" });
    await user.click(dialogConfirm);

    await waitFor(() => {
      expect(commands.planExecute).toHaveBeenCalledWith("plan-1", ["i1"], "auto_rename");
    });
    expect(await screen.findByText(/Executed 1 change/i)).toBeInTheDocument();
  });
});
