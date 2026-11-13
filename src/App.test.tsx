import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { useSettings } from "@/stores/settings";
import { defaultSettings } from "@/test/fixtures";
import { App } from "./App";

vi.mock("@/bindings", () => ({
  commands: { settingsGet: vi.fn(), settingsUpdate: vi.fn(), appInfo: vi.fn() },
}));
const mocked = vi.mocked(commands);

beforeEach(() => {
  useSettings.setState({ settings: null, status: "idle", saving: false, loadError: null, saveError: null });
});

describe("App bootstrap", () => {
  it("loads settings, applies saved appearance, then renders the shell", async () => {
    const s = defaultSettings();
    s.appearance = { theme: "dark", density: "compact" };
    mocked.settingsGet.mockResolvedValue({ status: "ok", data: s });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(document.documentElement).toHaveAttribute("data-density", "compact");
  });

  it("shows a recoverable error if settings cannot be loaded", async () => {
    mocked.settingsGet
      .mockResolvedValueOnce({ status: "error", error: { code: "database", message: "DB unavailable", hint: null, field: null } })
      .mockResolvedValueOnce({ status: "ok", data: defaultSettings() });
    render(<App />);
    expect(await screen.findByRole("alert")).toHaveTextContent("DB unavailable");
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  });
});
