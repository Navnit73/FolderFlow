import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/bindings";
import { defaultSettings } from "@/test/fixtures";
import type { Settings } from "@/types/settings";
import { useSettings } from "./settings";

vi.mock("@/bindings", () => ({
  commands: { settingsGet: vi.fn(), settingsUpdate: vi.fn(), appInfo: vi.fn() },
}));
const mocked = vi.mocked(commands);

type Reply = { status: "ok"; data: Settings } | { status: "error"; error: { code: "database"; message: string; hint: null; field: null } };

function deferred() {
  let resolve!: (v: Reply) => void;
  const promise = new Promise<Reply>((r) => (resolve = r));
  return { promise, resolve };
}

beforeEach(async () => {
  useSettings.setState({ settings: null, status: "idle", saving: false, loadError: null, saveError: null });
  mocked.settingsGet.mockResolvedValue({ status: "ok", data: defaultSettings() });
  await useSettings.getState().load();
});

describe("settings store", () => {
  it("BUG-004: a failed save that resolves after a later successful save does not roll back to stale state", async () => {
    const first = deferred();
    const second = deferred();
    mocked.settingsUpdate.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    const store = useSettings.getState();
    const p1 = store.update((s) => ({ ...s, appearance: { ...s.appearance, theme: "dark" } }));
    const p2 = useSettings.getState().update((s) => ({ ...s, appearance: { ...s.appearance, density: "compact" } }));

    // Backend truth after the second (successful) save: dark + compact.
    const persisted = defaultSettings();
    persisted.appearance = { theme: "dark", density: "compact" };
    mocked.settingsGet.mockResolvedValue({ status: "ok", data: persisted });

    second.resolve({ status: "ok", data: persisted });
    await p2;
    first.resolve({ status: "error", error: { code: "database", message: "busy", hint: null, field: null } });
    await p1;

    expect(useSettings.getState().settings?.appearance).toEqual({ theme: "dark", density: "compact" });
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(document.documentElement).toHaveAttribute("data-density", "compact");
    expect(useSettings.getState().saveError?.message).toBe("busy");
    expect(useSettings.getState().saving).toBe(false);
  });

  it("a successful save clears a previous save error", async () => {
    mocked.settingsUpdate
      .mockResolvedValueOnce({ status: "error", error: { code: "database", message: "busy", hint: null, field: null } })
      .mockImplementationOnce(async (s) => ({ status: "ok", data: s as Settings }));
    await useSettings.getState().update((s) => ({ ...s, appearance: { ...s.appearance, theme: "dark" } }));
    expect(useSettings.getState().saveError).not.toBeNull();
    await useSettings.getState().update((s) => ({ ...s, appearance: { ...s.appearance, theme: "light" } }));
    expect(useSettings.getState().saveError).toBeNull();
    expect(useSettings.getState().settings?.appearance.theme).toBe("light");
  });
});
