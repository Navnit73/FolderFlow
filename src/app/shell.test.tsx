import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { useSettings } from "@/stores/settings";
import { appInfoFixture, defaultSettings } from "@/test/fixtures";
import { NAV_ITEMS } from "./navigation";
import { routes } from "./routes";

vi.mock("@/bindings", () => ({
  commands: { settingsGet: vi.fn(), settingsUpdate: vi.fn(), appInfo: vi.fn() },
}));

const mocked = vi.mocked(commands);

async function renderAt(path: string) {
  useSettings.setState({ settings: null, status: "idle", saving: false, loadError: null, saveError: null });
  await useSettings.getState().load();
  const router = createMemoryRouter(routes, { initialEntries: [path] });
  render(<RouterProvider router={router} />);
  return router;
}

beforeEach(() => {
  mocked.settingsGet.mockResolvedValue({ status: "ok", data: defaultSettings() });
  mocked.settingsUpdate.mockImplementation(async (s) => ({ status: "ok", data: s as ReturnType<typeof defaultSettings> }));
  mocked.appInfo.mockResolvedValue({ status: "ok", data: appInfoFixture });
});

describe("UI-SHELL-01 sidebar navigation", () => {
  it("lists every screen in order; unavailable screens are disabled, not fake", async () => {
    await renderAt("/");
    const nav = screen.getByRole("navigation", { name: "Main" });
    const labels = NAV_ITEMS.map((i) => i.label);
    for (const label of labels) expect(within(nav).getByText(label)).toBeInTheDocument();

    for (const item of NAV_ITEMS.filter((i) => !i.available)) {
      expect(within(nav).getByText(item.label).closest("[aria-disabled='true']")).not.toBeNull();
      expect(within(nav).queryByRole("link", { name: new RegExp(item.label) })).toBeNull();
    }
  });

  it("redirects / and unknown routes to the first available screen", async () => {
    const router = await renderAt("/does-not-exist");
    expect(router.state.location.pathname).toBe("/settings/appearance");
    expect(screen.getByRole("link", { name: /Settings/ })).toHaveAttribute("aria-current", "page");
  });

  it("navigates between settings sections", async () => {
    const router = await renderAt("/settings/appearance");
    await userEvent.click(screen.getByRole("link", { name: "About" }));
    expect(router.state.location.pathname).toBe("/settings/about");
    expect(await screen.findByText(appInfoFixture.dataDir)).toBeInTheDocument();
  });
});

describe("UI-SHELL-02 page header", () => {
  it("renders the page title as the top-level heading and a drag region", async () => {
    await renderAt("/settings/appearance");
    const h1 = screen.getByRole("heading", { level: 1, name: "Settings" });
    expect(h1.closest("header")).toHaveAttribute("data-tauri-drag-region");
  });
});

describe("UI-SHELL-03 theme switch", () => {
  it("applies the theme immediately and persists it through settings_update", async () => {
    await renderAt("/settings/appearance");
    const group = screen.getByRole("radiogroup", { name: "Appearance" });
    await userEvent.click(within(group).getByRole("radio", { name: "Dark" }));

    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(within(group).getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
    expect(mocked.settingsUpdate).toHaveBeenCalledTimes(1);
    expect(mocked.settingsUpdate.mock.calls[0]?.[0].appearance).toEqual({ theme: "dark", density: "comfortable" });
  });

  it("applies density and supports arrow-key selection", async () => {
    await renderAt("/settings/appearance");
    const group = screen.getByRole("radiogroup", { name: "Density" });
    within(group).getByRole("radio", { name: "Comfortable" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(document.documentElement).toHaveAttribute("data-density", "compact");
  });

  it("reverts and shows an error when saving fails", async () => {
    mocked.settingsUpdate.mockResolvedValueOnce({
      status: "error",
      error: { code: "database", message: "Couldn't save settings.", hint: "Restart FolderFlow.", field: null },
    });
    await renderAt("/settings/appearance");
    await userEvent.click(screen.getByRole("radio", { name: "Light" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't save settings.");
    expect(document.documentElement).not.toHaveAttribute("data-theme");
    expect(screen.getByRole("radio", { name: "System" })).toHaveAttribute("aria-checked", "true");
  });
});

describe("About section", () => {
  it("shows an error with retry when app_info fails", async () => {
    mocked.appInfo.mockResolvedValueOnce({
      status: "error",
      error: { code: "database", message: "FolderFlow couldn't read its database.", hint: null, field: null },
    });
    await renderAt("/settings/about");
    expect(await screen.findByRole("alert")).toHaveTextContent("couldn't read its database");
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText(appInfoFixture.logDir)).toBeInTheDocument();
  });
});
