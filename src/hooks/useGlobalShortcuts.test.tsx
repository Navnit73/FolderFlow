import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { routes } from "@/app/routes";
import { useActivity } from "@/stores/activity";
import { useCategories } from "@/stores/categories";
import { useDashboard } from "@/stores/dashboard";
import { useDuplicates } from "@/stores/duplicates";
import { useFolders } from "@/stores/folders";
import { useLicense } from "@/stores/license";
import { usePreview } from "@/stores/preview";
import { useRules } from "@/stores/rules";
import { useSchedules } from "@/stores/schedules";
import { useSettings } from "@/stores/settings";
import { defaultSettings } from "@/test/fixtures";
import { commands, resetBindingMocks } from "@/test/mockBindings";

vi.mock("@/bindings", () => import("@/test/mockBindings"));

async function renderAppAt(path: string) {
  useSettings.setState({ settings: defaultSettings(), status: "ready", saving: false, loadError: null, saveError: null });
  useActivity.getState().reset();
  useCategories.getState().reset();
  useDashboard.getState().reset();
  useDuplicates.getState().reset();
  useFolders.getState().reset();
  useLicense.getState().reset();
  usePreview.getState().reset();
  useRules.getState().reset();
  useSchedules.getState().reset();

  const router = createMemoryRouter(routes, { initialEntries: [path] });
  render(<RouterProvider router={router} />);
  await waitFor(() => expect(screen.getByRole("navigation", { name: "Main" })).toBeInTheDocument());
  return router;
}

function dispatchShortcut(
  target: EventTarget,
  key: string,
  opts: { metaKey?: boolean; ctrlKey?: boolean; shiftKey?: boolean; repeat?: boolean } = {},
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key,
    metaKey: opts.metaKey ?? true,
    ctrlKey: opts.ctrlKey ?? false,
    shiftKey: opts.shiftKey ?? false,
    repeat: opts.repeat ?? false,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    target.dispatchEvent(event);
  });
  return event;
}

beforeEach(() => {
  resetBindingMocks();
});

describe("useGlobalShortcuts (UI-KEYS-01..02, FR-220)", () => {
  it("UI-KEYS-01: navigates via ⌘N, ⌘O, ⇧⌘O, ⌘, and triggers addWithPicker on ⇧⌘O", async () => {
    const router = await renderAppAt("/overview");

    dispatchShortcut(document, "n");
    await waitFor(() => expect(router.state.location.pathname).toBe("/rules/new"));

    dispatchShortcut(document, "o");
    await waitFor(() => expect(router.state.location.pathname).toBe("/folders"));

    dispatchShortcut(document, "O", { shiftKey: true });
    await waitFor(() => expect(commands.foldersAddWithPicker).toHaveBeenCalledTimes(1));

    dispatchShortcut(document, ",");
    await waitFor(() => expect(router.state.location.pathname).toMatch(/^\/settings/));
  });

  it("⌘F focuses page search input and ⌘R refreshes active route store without reloading", async () => {
    await renderAppAt("/activity");
    const search = await screen.findByRole("searchbox", { name: "Search activity" });
    expect(search).toHaveAttribute("data-ff-search", "true");
    expect(document.activeElement).not.toBe(search);

    const fEvent = dispatchShortcut(document, "f");
    expect(fEvent.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(search);

    await waitFor(() => expect(commands.activityList).toHaveBeenCalled());
    const initialCalls = commands.activityList.mock.calls.length;

    const onRefresh = vi.fn();
    window.addEventListener("ff:refresh", onRefresh);
    try {
      const rEvent = dispatchShortcut(document, "r");
      expect(rEvent.defaultPrevented).toBe(true);
      await waitFor(() => expect(commands.activityList.mock.calls.length).toBeGreaterThan(initialCalls));
      expect(onRefresh).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener("ff:refresh", onRefresh);
    }
  });

  it("UI-KEYS-02: ⌘Z invokes last undo outside inputs, but never intercepts inside input or textarea", async () => {
    await renderAppAt("/activity");
    await waitFor(() => expect(commands.activityList).toHaveBeenCalled());

    const outsideEvent = dispatchShortcut(document, "z");
    expect(outsideEvent.defaultPrevented).toBe(true);
    await waitFor(() => expect(commands.activityUndo).toHaveBeenCalledWith({ kind: "last" }));

    commands.activityUndo.mockClear();

    const search = screen.getByRole("searchbox", { name: "Search activity" });
    search.focus();
    const inputEvent = dispatchShortcut(search, "z");
    expect(inputEvent.defaultPrevented).toBe(false);
    expect(commands.activityUndo).not.toHaveBeenCalled();

    const textarea = document.createElement("textarea");
    document.body.appendChild(textarea);
    try {
      textarea.focus();
      const textareaEvent = dispatchShortcut(textarea, "z");
      expect(textareaEvent.defaultPrevented).toBe(false);
      expect(commands.activityUndo).not.toHaveBeenCalled();
    } finally {
      textarea.remove();
    }
  });

  it("suppresses global shortcuts while an aria-modal dialog is open", async () => {
    const router = await renderAppAt("/overview");

    function ModalHarness() {
      const [open] = useState(true);
      return open ? <div role="dialog" aria-modal="true" aria-label="Test modal" /> : null;
    }

    const { unmount } = render(<ModalHarness />);
    try {
      const nEvent = dispatchShortcut(document, "n");
      const zEvent = dispatchShortcut(document, "z");
      const rEvent = dispatchShortcut(document, "r");

      expect(nEvent.defaultPrevented).toBe(false);
      expect(zEvent.defaultPrevented).toBe(false);
      expect(rEvent.defaultPrevented).toBe(false);
      expect(router.state.location.pathname).toBe("/overview");
      expect(commands.activityUndo).not.toHaveBeenCalled();
    } finally {
      unmount();
    }

    // Sanity check normal keyboard handling resumes once modal unmounts
    act(() => {
      fireEvent.keyDown(document, { key: "n", metaKey: true });
    });
    await waitFor(() => expect(router.state.location.pathname).toBe("/rules/new"));
  });
});
