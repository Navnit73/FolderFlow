import { useEffect } from "react";
import { useLocation, useNavigate } from "react-router";
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

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") {
    return true;
  }
  if (target.isContentEditable || target.getAttribute("role") === "textbox") {
    return true;
  }
  return target.closest("[contenteditable='true']") !== null;
}

function isModalDialogOpen(): boolean {
  return document.querySelector('[role="dialog"][aria-modal="true"]') !== null;
}

function refreshActiveRoute(pathname: string): void {
  if (pathname.startsWith("/folders")) {
    void useFolders.getState().refresh();
  } else if (pathname.startsWith("/rules")) {
    void useRules.getState().refresh();
  } else if (pathname.startsWith("/preview")) {
    void usePreview.getState().generate();
  } else if (pathname.startsWith("/activity")) {
    void useActivity.getState().refresh();
  } else if (pathname.startsWith("/duplicates")) {
    void useDuplicates.getState().load();
  } else if (pathname.startsWith("/scheduler")) {
    void useSchedules.getState().load();
  } else if (pathname.startsWith("/categories")) {
    void useCategories.getState().refresh();
  } else if (pathname.startsWith("/settings")) {
    void useSettings.getState().load();
  } else if (pathname.startsWith("/license")) {
    void useLicense.getState().refresh();
  } else {
    void useDashboard.getState().refresh();
  }
  window.dispatchEvent(new CustomEvent("ff:refresh"));
}

/**
 * Registers desktop macOS keyboard shortcuts defined in `FR-220`:
 * - `⌘N`: New rule
 * - `⇧⌘O`: Add folder (opens native picker on `/folders`)
 * - `⌘O`: Open Folders view
 * - `⌘F`: Focus active page search input
 * - `⌘Z`: Undo last file operation (never overrides text editing inside inputs)
 * - `⌘R`: Refresh active page data without reloading the WebView
 * - `⌘,`: Open Settings
 * (`⌘↩` for executing selected preview items lives inside `PreviewPage`.)
 */
export function useGlobalShortcuts(): void {
  const navigate = useNavigate();
  const { pathname } = useLocation();

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      if (isModalDialogOpen()) return;

      const editable = isEditableTarget(e.target);
      const key = e.key.toLowerCase();

      if (key === "z" && !e.shiftKey) {
        if (editable || e.repeat) return;
        e.preventDefault();
        void useActivity.getState().undo({ kind: "last" });
        return;
      }

      if (key === "n" && !e.shiftKey) {
        if (editable || e.repeat) return;
        e.preventDefault();
        void navigate("/rules/new");
        return;
      }

      if (key === "o" && e.shiftKey) {
        if (editable || e.repeat) return;
        e.preventDefault();
        void navigate("/folders");
        void useFolders.getState().addWithPicker();
        return;
      }

      if (key === "o" && !e.shiftKey) {
        if (editable || e.repeat) return;
        e.preventDefault();
        void navigate("/folders");
        return;
      }

      if (key === "f" && !e.shiftKey) {
        const searchInput = document.querySelector<HTMLInputElement | HTMLTextAreaElement>(
          "[data-ff-search]",
        );
        if (searchInput) {
          e.preventDefault();
          searchInput.focus();
          searchInput.select?.();
        }
        return;
      }

      if (key === "r" && !e.shiftKey) {
        e.preventDefault();
        if (!e.repeat) {
          refreshActiveRoute(pathname);
        }
        return;
      }

      if (e.key === "," && !e.shiftKey) {
        e.preventDefault();
        void navigate("/settings");
      }
    };

    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [navigate, pathname]);
}
