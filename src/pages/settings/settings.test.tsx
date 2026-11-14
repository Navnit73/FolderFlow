import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { routes } from "@/app/routes";
import { useCategories } from "@/stores/categories";
import { useFolders } from "@/stores/folders";
import { useRules } from "@/stores/rules";
import { useSettings } from "@/stores/settings";
import { commands, emitAppNotification, resetBindingMocks } from "@/test/mockBindings";

vi.mock("@/bindings", () => import("@/test/mockBindings"));

const ok = <T,>(data: T) => ({ status: "ok" as const, data });
const err = (message: string, code = "database") => ({
  status: "error" as const,
  error: { code, message, hint: null, field: null },
});

async function renderSettingsAt(section: string) {
  useSettings.setState({ settings: null, status: "idle", saving: false, loadError: null, saveError: null });
  useFolders.getState().reset();
  useRules.getState().reset();
  useCategories.getState().reset();
  await useSettings.getState().load();
  const router = createMemoryRouter(routes, { initialEntries: [`/settings/${section}`] });
  render(<RouterProvider router={router} />);
  return router;
}

beforeEach(() => {
  resetBindingMocks();
});

describe("Settings sections (Monitoring, Organization, Performance, Notifications, Privacy, Backup)", () => {
  it("MonitoringSection persists stability checks and ignore toggles", async () => {
    await renderSettingsAt("monitoring");
    await userEvent.click(document.getElementById("mon-ignore-temp") as HTMLElement);
    await waitFor(() =>
      expect(commands.settingsUpdate).toHaveBeenCalledWith(
        expect.objectContaining({
          monitoring: expect.objectContaining({ ignoreTemporaryFiles: false }),
        }),
      ),
    );
  });

  it("OrganizationSection previews naming template and updates conflict policy", async () => {
    await renderSettingsAt("organization");
    expect(await screen.findByText(/Preview: 2026-04-15_invoice\.pdf/i)).toBeInTheDocument();

    await userEvent.selectOptions(screen.getByLabelText("Conflict resolution policy"), "skip");
    await waitFor(() =>
      expect(commands.settingsUpdate).toHaveBeenCalledWith(
        expect.objectContaining({
          organization: expect.objectContaining({ conflictPolicy: "skip" }),
        }),
      ),
    );
  });

  it("PerformanceSection toggles hashOnScan and runs database VACUUM & ANALYZE", async () => {
    await renderSettingsAt("performance");
    await userEvent.click(document.getElementById("perf-hash-scan") as HTMLElement);
    await waitFor(() =>
      expect(commands.settingsUpdate).toHaveBeenCalledWith(
        expect.objectContaining({
          performance: expect.objectContaining({ hashOnScan: true }),
        }),
      ),
    );

    await userEvent.click(screen.getByRole("button", { name: /Run VACUUM & ANALYZE/i }));
    expect(commands.dbMaintenance).toHaveBeenCalled();
    expect(await screen.findByText(/Database compacted:/i)).toBeInTheDocument();
  });

  it("NotificationsSection updates per-type preferences and sends test alert", async () => {
    commands.notificationsTest.mockResolvedValueOnce(
      ok({
        id: "n1",
        kind: "success_summary",
        severity: "info",
        title: "FolderFlow test notification",
        body: "Notifications are working.",
        createdAtMs: Date.now(),
        read: false,
        actionRoute: "/activity",
      }),
    );
    commands.notificationsList.mockResolvedValue(
      ok([
        {
          id: "n1",
          kind: "success_summary",
          severity: "info",
          title: "FolderFlow test notification",
          body: "Notifications are working.",
          createdAtMs: Date.now(),
          read: false,
          actionRoute: "/activity",
        },
      ]),
    );

    await renderSettingsAt("notifications");
    await userEvent.click(document.getElementById("notif-type-duplicatesFound") as HTMLElement);
    await waitFor(() =>
      expect(commands.settingsUpdate).toHaveBeenCalledWith(
        expect.objectContaining({
          notifications: expect.objectContaining({
            perType: expect.objectContaining({ duplicatesFound: false }),
          }),
        }),
      ),
    );

    await userEvent.click(screen.getByRole("button", { name: /Send test/i }));
    expect(await screen.findByText(/Delivered test alert/i)).toBeInTheDocument();
  });

  it("PrivacySection creates and deletes global exclusions", async () => {
    await renderSettingsAt("privacy");
    expect(await screen.findByText("No global exclusions configured.")).toBeInTheDocument();

    await userEvent.selectOptions(screen.getByLabelText("Exclusion type"), "extension");
    await userEvent.type(screen.getByLabelText("Exclusion pattern or path"), "iso");
    await userEvent.click(screen.getByRole("button", { name: /Add exclusion/i }));

    expect(commands.exclusionsCreate).toHaveBeenCalledWith({
      folderId: null,
      exclusionType: "extension",
      path: "iso",
    });
    expect(await screen.findByText("iso")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Remove exclusion iso" }));
    await waitFor(() => expect(commands.exclusionsDelete).toHaveBeenCalledWith("excl-1"));
  });

  it("BackupSection exports JSON, previews diff with disabled-rule safety count, and applies import", async () => {
    commands.configExport.mockResolvedValueOnce(ok('{"schemaVersion":1,"rules":[]}'));
    commands.configImportPreview.mockResolvedValueOnce(
      ok({
        schemaVersion: 1,
        exportedAt: Date.now(),
        settingsChanges: ["organization"],
        foldersToAdd: ["Downloads"],
        foldersToUpdate: [],
        foldersUnchanged: [],
        categoriesToAdd: ["Receipts"],
        categoriesToUpdate: [],
        exclusionsToAdd: ["*.bak"],
        exclusionsUnchanged: [],
        rulesToAdd: ["Archive Invoices"],
        rulesToUpdate: [],
        forcedDisabledRuleCount: 1,
        warnings: ["Skipped system root"],
      }),
    );
    commands.configImportApply.mockResolvedValueOnce(
      ok({
        mode: "merge",
        foldersImported: 1,
        categoriesImported: 1,
        exclusionsImported: 1,
        rulesImportedDisabled: 1,
        warnings: [],
      }),
    );

    await renderSettingsAt("backup");
    await userEvent.click(screen.getByRole("button", { name: /Export JSON backup/i }));
    await waitFor(() => expect(commands.configExport).toHaveBeenCalled());

    await userEvent.click(screen.getByRole("button", { name: "Preview diff" }));
    expect(await screen.findByText(/Import preview \(schema v1\)/i)).toBeInTheDocument();
    expect(screen.getByText(/1 imported rule will be created disabled/i)).toBeInTheDocument();
    expect(screen.getByText("Skipped system root")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Apply \(merge\)/i }));
    await waitFor(() =>
      expect(commands.configImportApply).toHaveBeenCalledWith('{"schemaVersion":1,"rules":[]}', "merge"),
    );
    expect(await screen.findByText(/all imported rules start disabled/i)).toBeInTheDocument();
  });

  it("BUG-033: PerformanceSection displays maintenance errors instead of rejecting unhandled", async () => {
    commands.dbMaintenance.mockResolvedValueOnce(err("SQLite database is locked"));
    await renderSettingsAt("performance");

    await userEvent.click(screen.getByRole("button", { name: /Run VACUUM & ANALYZE/i }));
    expect(await screen.findByRole("alert")).toHaveTextContent("SQLite database is locked");
    expect(screen.getByRole("button", { name: /Run VACUUM & ANALYZE/i })).toBeEnabled();
  });

  it("BUG-033: NotificationsSection updates live on appNotificationEvent, marks notifications read, and shows command errors", async () => {
    await renderSettingsAt("notifications");

    await act(async () => {
      emitAppNotification({
        id: "n-live",
        kind: "destination_unavailable",
        severity: "warning",
        title: "Destination unavailable",
        body: "ExternalSSD could not be reached",
        count: 1,
        batchCount: 1,
        folderId: "f-ext",
        createdAtMs: Date.now(),
        read: false,
      });
    });

    expect(await screen.findByText("ExternalSSD could not be reached")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Mark all read/i }));
    expect(commands.notificationsMarkRead).toHaveBeenCalledWith(null);

    commands.notificationsClear.mockResolvedValueOnce(err("Notification store failure"));
    await userEvent.click(screen.getByRole("button", { name: /Clear/i }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Notification store failure");
  });

  it("BUG-033: BackupSection updates preview for Replace mode, warns of overwrite, and rejects oversized uploads", async () => {
    commands.configExport.mockResolvedValueOnce(ok('{"schemaVersion":1,"rules":[]}'));
    commands.configImportPreview.mockResolvedValue(
      ok({
        schemaVersion: 1,
        exportedAt: Date.now(),
        settingsChanges: [],
        foldersToAdd: [],
        foldersToUpdate: [],
        foldersUnchanged: [],
        categoriesToAdd: [],
        categoriesToUpdate: [],
        exclusionsToAdd: [],
        exclusionsUnchanged: [],
        rulesToAdd: [],
        rulesToUpdate: [],
        forcedDisabledRuleCount: 0,
        warnings: [],
      }),
    );

    await renderSettingsAt("backup");
    await userEvent.click(screen.getByRole("button", { name: /Export JSON backup/i }));
    await userEvent.click(screen.getByRole("button", { name: "Preview diff" }));
    await screen.findByText(/Import preview \(schema v1\)/i);

    await userEvent.click(screen.getByRole("radio", { name: "Replace" }));
    await waitFor(() =>
      expect(commands.configImportPreview).toHaveBeenLastCalledWith('{"schemaVersion":1,"rules":[]}', "replace"),
    );
    expect(screen.getByText(/Replace mode removes existing folders, custom categories, exclusions, and rules/i)).toBeInTheDocument();

    const fileInput = document.querySelector('input[type="file"]') as HTMLInputElement;
    const huge = new File(["{}"], "huge.json", { type: "application/json" });
    Object.defineProperty(huge, "size", { value: 6 * 1024 * 1024 });
    await userEvent.upload(fileInput, huge);
    expect(await screen.findByRole("alert")).toHaveTextContent(/5 MB/i);
  });
});

