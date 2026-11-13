import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { routes } from "@/app/routes";
import { useFolders } from "@/stores/folders";
import { useSettings } from "@/stores/settings";
import {
  commands,
  emitFolderStatus,
  folderFixture,
  resetBindingMocks,
  runtimeFixture,
} from "@/test/mockBindings";

vi.mock("@/bindings", () => import("@/test/mockBindings"));

const ok = <T,>(data: T) => ({ status: "ok" as const, data });
const err = (message: string, code = "validation", field: string | null = null) => ({
  status: "error" as const,
  error: { code, message, hint: null, field },
});

async function renderAt(path: string) {
  useSettings.setState({ settings: null, status: "idle", saving: false, loadError: null, saveError: null });
  useFolders.getState().reset();
  await useSettings.getState().load();
  const router = createMemoryRouter(routes, { initialEntries: [path] });
  render(<RouterProvider router={router} />);
  return router;
}

beforeEach(() => resetBindingMocks());

describe("UI-FOLD-01 folder list", () => {
  it("shows an empty state that explains nothing is moved yet", async () => {
    await renderAt("/folders");
    expect(await screen.findByText("No folders yet")).toBeInTheDocument();
    expect(screen.getByText(/won't move anything/i)).toBeInTheDocument();
  });

  it("lists folders with live status, file count, size and path", async () => {
    commands.foldersList.mockResolvedValue(ok([folderFixture(), folderFixture({ id: "f2", name: "Desktop", path: "/Users/test/Desktop", fileCount: 1, totalBytes: 900 })]));
    commands.monitoringStatus.mockResolvedValue(ok({ paused: false, folders: [runtimeFixture(), runtimeFixture({ folderId: "f2", state: "permission_denied" })] }));
    await renderAt("/folders");
    const row = await screen.findByRole("row", { name: /Downloads/ });
    expect(within(row).getByText("Watching")).toBeInTheDocument();
    expect(within(row).getByText("42 files")).toBeInTheDocument();
    expect(within(row).getByText("1.5 MB")).toBeInTheDocument();
    expect(within(row).getByText("/Users/test/Downloads")).toBeInTheDocument();
    const desktop = screen.getByRole("row", { name: /Desktop/ });
    expect(within(desktop).getByText("No access")).toBeInTheDocument();
    expect(within(desktop).getByText("1 file")).toBeInTheDocument();
  });

  it("shows a recoverable error when folders can't be loaded", async () => {
    commands.foldersList.mockResolvedValueOnce(err("Database unavailable", "database"));
    await renderAt("/folders");
    expect(await screen.findByRole("alert")).toHaveTextContent("Database unavailable");
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("No folders yet")).toBeInTheDocument();
  });
});

describe("UI-FOLD-02 adding folders", () => {
  it("adds a well-known folder using the recursive default from settings", async () => {
    commands.foldersAddKnown.mockResolvedValue(ok(folderFixture()));
    await renderAt("/folders");
    await screen.findByText("No folders yet");
    await userEvent.click(screen.getAllByRole("button", { name: /Add folder/ })[0] as HTMLElement);
    await userEvent.click(screen.getByRole("menuitem", { name: "Downloads" }));
    expect(commands.foldersAddKnown).toHaveBeenCalledWith("downloads", false);
    expect(await screen.findByRole("row", { name: /Downloads/ })).toBeInTheDocument();
  });

  it("picker cancel changes nothing; add errors are shown", async () => {
    commands.foldersAddWithPicker.mockResolvedValueOnce(ok(null));
    commands.foldersAddWithPicker.mockResolvedValueOnce(err("\"Downloads\" is already monitored."));
    await renderAt("/folders");
    await screen.findByText("No folders yet");
    const open = async () => {
      await userEvent.click(screen.getAllByRole("button", { name: /Add folder/ })[0] as HTMLElement);
      await userEvent.click(screen.getByRole("menuitem", { name: /Choose folder/ }));
    };
    await open();
    expect(screen.getByText("No folders yet")).toBeInTheDocument();
    await open();
    expect(await screen.findByRole("alert")).toHaveTextContent("already monitored");
  });
});

describe("UI-FOLD-03 monitoring controls", () => {
  it("row actions pause, resume and rescan by folder id", async () => {
    commands.foldersList.mockResolvedValue(ok([folderFixture()]));
    commands.monitoringStatus.mockResolvedValue(ok({ paused: false, folders: [runtimeFixture()] }));
    await renderAt("/folders");
    const row = await screen.findByRole("row", { name: /Downloads/ });
    await userEvent.click(within(row).getByRole("button", { name: "Pause Downloads" }));
    expect(commands.foldersAction).toHaveBeenCalledWith("f1", "pause");
    await userEvent.click(within(row).getByRole("button", { name: "Rescan Downloads" }));
    expect(commands.foldersAction).toHaveBeenCalledWith("f1", "rescan");

    act(() => emitFolderStatus(runtimeFixture({ state: "paused" })));
    expect(await within(row).findByText("Paused")).toBeInTheDocument();
    await userEvent.click(within(row).getByRole("button", { name: "Resume Downloads" }));
    expect(commands.foldersAction).toHaveBeenCalledWith("f1", "resume");
  });

  it("global pause and resume", async () => {
    commands.foldersList.mockResolvedValue(ok([folderFixture()]));
    await renderAt("/folders");
    await screen.findByRole("row", { name: /Downloads/ });
    await userEvent.click(screen.getByRole("button", { name: "Pause all" }));
    expect(commands.monitoringSetPaused).toHaveBeenCalledWith(true);
    await userEvent.click(await screen.findByRole("button", { name: "Resume all" }));
    expect(commands.monitoringSetPaused).toHaveBeenCalledWith(false);
  });
});

describe("UI-FOLD-04 remove and edit", () => {
  it("remove asks for confirmation and states files are not touched", async () => {
    commands.foldersList.mockResolvedValue(ok([folderFixture()]));
    await renderAt("/folders");
    const row = await screen.findByRole("row", { name: /Downloads/ });
    await userEvent.click(within(row).getByRole("button", { name: "Remove Downloads" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent(/files stay where they are/i);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(commands.foldersRemove).not.toHaveBeenCalled();
    await userEvent.click(within(row).getByRole("button", { name: "Remove Downloads" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Stop monitoring" }));
    expect(commands.foldersRemove).toHaveBeenCalledWith("f1");
    await waitFor(() => expect(screen.queryByRole("row", { name: /Downloads/ })).toBeNull());
  });

  it("detail page saves edited settings and shows field errors", async () => {
    commands.foldersList.mockResolvedValue(ok([folderFixture()]));
    commands.foldersUpdate
      .mockResolvedValueOnce(err("\"p/df\" is not a valid extension.", "validation", "excludeExtensions"))
      .mockImplementationOnce(async (_id: string, patch: { name: string }) => ok(folderFixture({ name: patch.name })));
    commands.foldersSummary.mockResolvedValue(ok({ files: 42, bytes: 1_500_000, byCategory: [{ category: "documents", files: 40, bytes: 1_400_000 }] }));
    await renderAt("/folders/f1");
    expect(await screen.findByRole("heading", { level: 1, name: "Downloads" })).toBeInTheDocument();
    expect(await screen.findByText("40 files")).toBeInTheDocument();

    const name = screen.getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Inbox");
    await userEvent.type(screen.getByLabelText("Skip these extensions"), "p/df");
    await userEvent.click(screen.getByRole("switch", { name: "Include subfolders" }));
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    expect(await screen.findByText("\"p/df\" is not a valid extension.")).toBeInTheDocument();

    const ext = screen.getByLabelText("Skip these extensions");
    await userEvent.clear(ext);
    await userEvent.type(ext, "dmg, .ZIP");
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    expect(commands.foldersUpdate).toHaveBeenLastCalledWith("f1", expect.objectContaining({
      name: "Inbox",
      recursive: true,
      settings: expect.objectContaining({ excludeExtensions: ["dmg", ".ZIP"] }),
    }));
    expect(await screen.findByRole("heading", { level: 1, name: "Inbox" })).toBeInTheDocument();
  });

  it("BUG-012: last-scan dates keep their capitalisation", async () => {
    commands.foldersList.mockResolvedValue(ok([folderFixture({ lastScannedAt: new Date(2026, 0, 15).getTime() })]));
    await renderAt("/folders/f1");
    const overview = await screen.findByRole("region", { name: "Overview" });
    expect(overview).toHaveTextContent(new Date(2026, 0, 15).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" }));
  });

  it("permission problems offer to open System Settings", async () => {
    commands.foldersList.mockResolvedValue(ok([folderFixture({ health: "permission_denied", lastError: "No permission" })]));
    commands.monitoringStatus.mockResolvedValue(ok({ paused: false, folders: [runtimeFixture({ state: "permission_denied", lastError: "FolderFlow no longer has permission" })] }));
    await renderAt("/folders/f1");
    await userEvent.click(await screen.findByRole("button", { name: "Open System Settings" }));
    expect(commands.openPrivacySettings).toHaveBeenCalled();
  });
});
