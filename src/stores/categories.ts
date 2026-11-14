import { create } from "zustand";
import { categoriesService, type Category, type CategoryInput } from "@/services/categories";
import { toUiError } from "@/services/result";
import type { UiError } from "@/types/settings";

type Status = "idle" | "loading" | "ready" | "error";

export type CategorySaveResult = { ok: true; category: Category } | { ok: false; error: UiError };

const sortCategories = (items: Category[]): Category[] =>
  [...items].sort((a, b) => a.priority - b.priority || Number(a.builtin) - Number(b.builtin) || a.name.localeCompare(b.name));

interface CategoriesState {
  categories: Category[];
  status: Status;
  loadError: UiError | null;
  actionError: UiError | null;
  load: () => Promise<void>;
  refresh: () => Promise<void>;
  create: (input: CategoryInput) => Promise<CategorySaveResult>;
  update: (id: string, input: CategoryInput) => Promise<CategorySaveResult>;
  remove: (id: string) => Promise<boolean>;
  setEnabled: (id: string, enabled: boolean) => Promise<void>;
  moveCategory: (id: string, direction: "up" | "down") => Promise<void>;
  clearActionError: () => void;
  reset: () => void;
}

const initial = {
  categories: [] as Category[],
  status: "idle" as Status,
  loadError: null as UiError | null,
  actionError: null as UiError | null,
};

export const useCategories = create<CategoriesState>((set, get) => {
  const upsert = (cat: Category) =>
    set((s) => ({
      categories: sortCategories(
        s.categories.some((c) => c.id === cat.id) ? s.categories.map((c) => (c.id === cat.id ? cat : c)) : [...s.categories, cat],
      ),
    }));

  const runAction = async (fn: () => Promise<void>) => {
    set({ actionError: null });
    try {
      await fn();
    } catch (e) {
      set({ actionError: toUiError(e) });
      void get().refresh();
    }
  };

  return {
    ...initial,

    load: async () => {
      set({ status: "loading", loadError: null });
      try {
        const list = await categoriesService.list();
        set({ categories: sortCategories(list), status: "ready" });
      } catch (e) {
        set({ status: "error", loadError: toUiError(e) });
      }
    },

    refresh: async () => {
      try {
        const list = await categoriesService.list();
        set({ categories: sortCategories(list), status: "ready" });
      } catch {
        // Keep last known list on background refresh failure.
      }
    },

    create: async (input) => {
      try {
        const created = await categoriesService.create(input);
        upsert(created);
        return { ok: true, category: created };
      } catch (e) {
        return { ok: false, error: toUiError(e) };
      }
    },

    update: async (id, input) => {
      try {
        const updated = await categoriesService.update(id, input);
        upsert(updated);
        return { ok: true, category: updated };
      } catch (e) {
        return { ok: false, error: toUiError(e) };
      }
    },

    remove: async (id) => {
      let ok = false;
      await runAction(async () => {
        await categoriesService.remove(id);
        set((s) => ({ categories: s.categories.filter((c) => c.id !== id) }));
        ok = true;
      });
      return ok;
    },

    setEnabled: async (id, enabled) => {
      await runAction(async () => {
        const updated = await categoriesService.setEnabled(id, enabled);
        upsert(updated);
      });
    },

    moveCategory: async (id, direction) => {
      const current = get().categories;
      const idx = current.findIndex((c) => c.id === id);
      if (idx < 0) return;
      const swapIdx = direction === "up" ? idx - 1 : idx + 1;
      if (swapIdx < 0 || swapIdx >= current.length) return;
      const next = [...current];
      const moving = next[idx];
      const other = next[swapIdx];
      if (!moving || !other) return;
      next[idx] = other;
      next[swapIdx] = moving;
      await runAction(async () => {
        const ordered = await categoriesService.reorder(next.map((c) => c.id));
        set({ categories: sortCategories(ordered) });
      });
    },

    clearActionError: () => set({ actionError: null }),

    reset: () => set(initial),
  };
});

