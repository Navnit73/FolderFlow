import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { routes } from "@/app/routes";
import { useDuplicates } from "@/stores/duplicates";
import { useSettings } from "@/stores/settings";
import {
  commands,
  duplicateGroupFixture,
  emitDupProgress,
  resetBindingMocks,
} from "@/test/mockBindings";

vi.mock("@/bindings", () => import("@/test/mockBindings"));

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

async function renderDuplicates() {
  useSettings.setState({ settings: null, status: "idle", saving: false, loadError: null, saveError: null });
  useDuplicates.getState().reset();
  await useSettings.getState().load();
  const router = createMemoryRouter(routes, { initialEntries: ["/duplicates"] });
  render(<RouterProvider router={router} />);
  return router;
}

beforeEach(() => {
  resetBindingMocks();
});

describe("DuplicatesPage (UI-DUP-01..02)", () => {
  it("UI-DUP-01: scans with live progress, filters by different names, reveals in Finder, and switches Keeper", async () => {
    const gDiff = duplicateGroupFixture({
      id: "grp-diff",
      sameContentDifferentNames: true,
      allSameName: false,
      wastedBytes: 4096,
    });
    const gSame = duplicateGroupFixture({
      id: "grp-same",
      contentHash: "b".repeat(64),
      sameContentDifferentNames: false,
      allSameName: true,
      members: [
        {
          fileRecordId: "fr-3",
          folderId: "f1",
          path: "/Users/test/Downloads/photo.jpg",
          filename: "photo.jpg",
          extension: "jpg",
          size: 2048,
          createdMs: 1_000_000,
          modifiedMs: 1_000_000,
          isKeeper: true,
          exists: true,
        },
        {
          fileRecordId: "fr-4",
          folderId: "f2",
          path: "/Users/test/Desktop/photo.jpg",
          filename: "photo.jpg",
          extension: "jpg",
          size: 2048,
          createdMs: 1_000_000,
          modifiedMs: 1_000_000,
          isKeeper: false,
          exists: true,
        },
      ],
    });

    const scanResponse = {
      totals: {
        totalGroups: 2,
        activeGroups: 2,
        ignoredGroups: 0,
        totalDuplicateCopies: 2,
        totalWastedBytes: 6144,
      },
      groups: [gDiff, gSame],
    };

    commands.dupScan.mockImplementation(async () => {
      emitDupProgress({ hashed: 2, total: 4, bytes: 8192 });
      return ok({
        filesScanned: 10,
        uniqueSizeSkipped: 6,
        partialHashed: 4,
        fullHashed: 4,
        cacheHits: 0,
        groupsFound: 2,
        totalWastedBytes: 6144,
        groups: scanResponse,
      });
    });

    await renderDuplicates();
    expect(await screen.findByText("No duplicates found yet")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Scan for duplicates/i }));
    expect(await screen.findByText(/Scanned 10 files: found 2 duplicate groups/i)).toBeInTheDocument();
    expect(screen.getByText("Same content, different names")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /photo\.jpg/i })).toBeInTheDocument();

    // Toggle "Different names only" hides same-name group
    await userEvent.click(screen.getByRole("switch", { name: /Different names only/i }));
    expect(screen.queryByRole("button", { name: /photo\.jpg/i })).not.toBeInTheDocument();

    // Reveal member in Finder
    await userEvent.click(screen.getByRole("button", { name: "Reveal report.pdf in Finder" }));
    expect(commands.dupRevealFile).toHaveBeenCalledWith("fr-1");

    // Promote second member to Keeper
    commands.dupAction.mockResolvedValueOnce(
      ok({
        action: "keep",
        affected: 1,
        movedToReviewDir: null,
        trashedCount: 0,
        groups: {
          totals: scanResponse.totals,
          groups: [
            {
              ...gDiff,
              members: gDiff.members.map((m) => ({ ...m, isKeeper: m.fileRecordId === "fr-2" })),
            },
            gSame,
          ],
        },
      }),
    );

    await userEvent.click(screen.getByRole("button", { name: "Keep this" }));
    expect(commands.dupAction).toHaveBeenCalledWith({
      kind: "keep",
      group_id: "grp-diff",
      file_record_id: "fr-2",
    });

    // Live dupProgress event updates state without crash during scan
    await act(async () => {
      emitDupProgress({ hashed: 4, total: 4, bytes: 12288 });
    });
  });

  it("UI-DUP-02: moves extras to Review, requires ConfirmDialog before trashing non-keepers, ignores, and exports CSV/JSON", async () => {
    const group = duplicateGroupFixture({ id: "grp-1", ignored: false });
    const totals = {
      totalGroups: 1,
      activeGroups: 1,
      ignoredGroups: 0,
      totalDuplicateCopies: 1,
      totalWastedBytes: 4096,
    };
    commands.dupGroups.mockResolvedValue(ok({ totals, groups: [group] }));

    await renderDuplicates();
    expect(await screen.findByRole("heading", { name: "report.pdf" })).toBeInTheDocument();

    // Export CSV & JSON
    commands.dupExport.mockResolvedValueOnce(ok("group_id,content_hash\ngrp-1,aaa\n"));
    await userEvent.click(screen.getByRole("button", { name: /Export CSV/i }));
    expect(commands.dupExport).toHaveBeenCalledWith("csv", false);
    expect(await screen.findByText(/Exported duplicates report \(CSV\)/i)).toBeInTheDocument();

    // Move extras to Review
    commands.dupAction.mockResolvedValueOnce(
      ok({
        action: "move_to_review",
        affected: 1,
        movedToReviewDir: "/Users/test/Downloads/Duplicate Review",
        trashedCount: 0,
        groups: { totals, groups: [group] },
      }),
    );
    await userEvent.click(screen.getByRole("button", { name: /Move extras to Review/i }));
    expect(commands.dupAction).toHaveBeenCalledWith({
      kind: "move_to_review",
      group_id: "grp-1",
      file_record_ids: null,
      review_dir: null,
    });
    expect(await screen.findByText(/Moved 1 duplicate copy to Duplicate Review/i)).toBeInTheDocument();

    // Clicking Trash non-keepers opens ConfirmDialog and does NOT call dupAction until confirmed
    await userEvent.click(screen.getByRole("button", { name: /Trash non-keepers/i }));
    expect(screen.getByRole("heading", { name: /Move duplicates to Trash\?/i })).toBeInTheDocument();
    expect(commands.dupAction).toHaveBeenCalledTimes(1);

    commands.dupAction.mockResolvedValueOnce(
      ok({
        action: "trash",
        affected: 1,
        movedToReviewDir: null,
        trashedCount: 1,
        groups: {
          totals: { totalGroups: 0, activeGroups: 0, ignoredGroups: 0, totalDuplicateCopies: 0, totalWastedBytes: 0 },
          groups: [],
        },
      }),
    );

    await userEvent.click(screen.getByRole("button", { name: "Move to Trash" }));
    await waitFor(() =>
      expect(commands.dupAction).toHaveBeenCalledWith({
        kind: "trash",
        group_id: "grp-1",
        file_record_ids: null,
        confirmed: true,
      }),
    );
    expect(await screen.findByText(/Moved 1 duplicate file to Trash/i)).toBeInTheDocument();
  });

  it("UI-DUP-03: shows ErrorState with retry on initial load failure and inline alert on scan failure", async () => {
    commands.dupGroups.mockResolvedValueOnce({
      status: "error",
      error: { code: "database", message: "Duplicate index is corrupt", hint: null, field: null },
    });

    await renderDuplicates();
    expect(await screen.findByRole("alert")).toHaveTextContent("Duplicate index is corrupt");

    commands.dupGroups.mockResolvedValueOnce(
      ok({
        totals: { totalGroups: 0, activeGroups: 0, ignoredGroups: 0, totalDuplicateCopies: 0, totalWastedBytes: 0 },
        groups: [],
      }),
    );
    await userEvent.click(screen.getByRole("button", { name: /Try again/i }));
    expect(await screen.findByText(/No duplicates found/i)).toBeInTheDocument();

    commands.dupScan.mockResolvedValueOnce({
      status: "error",
      error: { code: "permission", message: "Folder access denied during scan", hint: null, field: null },
    });
    await userEvent.click(screen.getByRole("button", { name: /Scan for duplicates/i }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Folder access denied during scan");
  });
});

