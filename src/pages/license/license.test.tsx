import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { routes } from "@/app/routes";
import { useActivity } from "@/stores/activity";
import { useDashboard } from "@/stores/dashboard";
import { useFolders } from "@/stores/folders";
import { useLicense } from "@/stores/license";
import { useSettings } from "@/stores/settings";
import { defaultLicenseStatus, defaultSettings } from "@/test/fixtures";
import { commands, emitTrayNavigate, resetBindingMocks } from "@/test/mockBindings";

vi.mock("@/bindings", () => import("@/test/mockBindings"));

const ok = <T,>(data: T) => ({ status: "ok" as const, data });
const err = (message: string, code: "validation" | "database" = "validation") => ({
  status: "error" as const,
  error: {
    code,
    message,
    hint: "Paste an offline signed key (FF1…) or import a .fflicense file.",
    field: "licenseKey",
  },
});

async function renderAppAt(path: string) {
  useSettings.setState({ settings: defaultSettings(), status: "ready", saving: false, loadError: null, saveError: null });
  useLicense.getState().reset();
  useDashboard.getState().reset();
  useFolders.getState().reset();
  useActivity.getState().reset();
  const router = createMemoryRouter(routes, { initialEntries: [path] });
  render(<RouterProvider router={router} />);
  return router;
}

beforeEach(() => {
  resetBindingMocks();
});

describe("LicensePage (UI-LIC-01) and Sidebar/Tray integration", () => {
  it("renders unlicensed state cleanly with Keychain and offline guarantees", async () => {
    await renderAppAt("/license");
    expect(await screen.findByRole("heading", { level: 2, name: "No active license" })).toBeInTheDocument();
    expect(screen.getByText(/Works Forever Offline/i)).toBeInTheDocument();
    expect(screen.getByText(/Stored in macOS Keychain/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Deactivate Mac" })).toBeNull();
  });

  it("activates an offline Ed25519 key (FF1.*), updates Sidebar badge, and clears the input", async () => {
    await renderAppAt("/license");
    const input = await screen.findByLabelText("License key or offline signed key");
    await userEvent.type(input, "FF1.eyJpZCI6ImRlbW8ifQ.c2ln");
    await userEvent.click(screen.getByRole("button", { name: "Activate License" }));

    expect(commands.licenseActivate).toHaveBeenCalledWith("FF1.eyJpZCI6ImRlbW8ifQ.c2ln");
    expect(await screen.findByRole("heading", { level: 2, name: "FolderFlow is licensed" })).toBeInTheDocument();
    expect(screen.getByText("Lifetime")).toBeInTheDocument();
    expect(screen.getByText("Never expires")).toBeInTheDocument();
    expect(screen.getByText("FF-LIFE-2026-0001")).toBeInTheDocument();
    expect((input as HTMLInputElement).value).toBe("");

    const sidebar = screen.getByRole("complementary");
    expect(within(sidebar).getByRole("link", { name: /Licensed ✓/i })).toHaveAttribute("href", "/license");
  });

  it("imports a .fflicense file via picker and preserves valid license on failed replacement", async () => {
    commands.licensePickAndActivateOfflineFile.mockResolvedValueOnce(
      ok(
        defaultLicenseStatus({
          state: "active_offline",
          licensed: true,
          edition: "pro",
          source: "offline_key",
          licenseId: "FF-FILE-2026-0009",
          maskedKey: "FF1.••••-file",
          activatedAtMs: Date.UTC(2026, 10, 14),
          expiresAtMs: Date.UTC(2027, 10, 14),
        }),
      ),
    );

    await renderAppAt("/license");
    await screen.findByRole("heading", { level: 2, name: "No active license" });
    await userEvent.click(screen.getByRole("button", { name: /Import \.fflicense File/i }));

    expect(commands.licensePickAndActivateOfflineFile).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("heading", { level: 2, name: "FolderFlow is licensed" })).toBeInTheDocument();
    expect(screen.getByText("FF-FILE-2026-0009")).toBeInTheDocument();

    // Failed replacement key shows an inline error alert while preserving the active license
    commands.licenseActivate.mockResolvedValueOnce(err("Invalid license signature"));
    const input = screen.getByLabelText("License key or offline signed key");
    await userEvent.type(input, "BAD-KEY-1234");
    await userEvent.click(screen.getByRole("button", { name: "Activate License" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Invalid license signature");
    expect(screen.getByRole("heading", { level: 2, name: "FolderFlow is licensed" })).toBeInTheDocument();
    expect(screen.getByText("FF-FILE-2026-0009")).toBeInTheDocument();
  });

  it("confirms before deactivating Mac and cancels safely", async () => {
    commands.licenseStatus.mockResolvedValue(
      ok(
        defaultLicenseStatus({
          state: "active_offline",
          licensed: true,
          edition: "lifetime",
          source: "offline_key",
          licenseId: "FF-LIFE-2026-0001",
        }),
      ),
    );

    await renderAppAt("/license");
    const deactivateBtn = await screen.findByRole("button", { name: "Deactivate Mac" });
    await userEvent.click(deactivateBtn);

    const dialog = await screen.findByRole("dialog", { name: "Deactivate this Mac?" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(commands.licenseDeactivate).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { level: 2, name: "FolderFlow is licensed" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Deactivate Mac" }));
    const confirmDialog = await screen.findByRole("dialog", { name: "Deactivate this Mac?" });
    await userEvent.click(within(confirmDialog).getByRole("button", { name: "Deactivate License" }));

    expect(commands.licenseDeactivate).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("heading", { level: 2, name: "No active license" })).toBeInTheDocument();
  });

  it("updates Sidebar badge and navigates when tray-navigate event fires", async () => {
    const router = await renderAppAt("/overview");
    const sidebar = screen.getByRole("complementary");
    expect(await within(sidebar).findByRole("link", { name: /Unlicensed/i })).toHaveAttribute("href", "/license");

    await act(async () => {
      emitTrayNavigate({ path: "/license" });
    });
    await waitFor(() => expect(router.state.location.pathname).toBe("/license"));

    await act(async () => {
      emitTrayNavigate({ path: "/activity" });
    });
    await waitFor(() => expect(router.state.location.pathname).toBe("/activity"));
  });
});

