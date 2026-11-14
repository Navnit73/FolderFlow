import { create } from "zustand";
import {
  rulesService,
  type Rule,
  type RuleDraftInput,
  type RuleTemplate,
  type SmartOrganizerCatalog,
  type SmartOrganizerSpec,
} from "@/services/rules";
import { toUiError } from "@/services/result";
import type { UiError } from "@/types/settings";

type Status = "idle" | "loading" | "ready" | "error";
export type RuleSaveResult = { ok: true; rule: Rule } | { ok: false; error: UiError };

interface RulesState {
  rules: Rule[];
  presets: RuleTemplate[];
  smartCatalog: SmartOrganizerCatalog | null;
  status: Status;
  loadError: UiError | null;
  actionError: UiError | null;
  load: () => Promise<void>;
  refresh: () => Promise<void>;
  create: (draft: RuleDraftInput) => Promise<RuleSaveResult>;
  createFromPreset: (presetId: string) => Promise<Rule | null>;
  createSmartPreset: (spec: SmartOrganizerSpec) => Promise<Rule | null>;
  createSmartPresetResult: (spec: SmartOrganizerSpec) => Promise<RuleSaveResult>;
  update: (id: string, draft: RuleDraftInput) => Promise<RuleSaveResult>;
  remove: (id: string) => Promise<boolean>;
  setEnabled: (id: string, enabled: boolean) => Promise<void>;
  reorder: (orderedIds: string[]) => Promise<void>;
  moveRule: (id: string, direction: "up" | "down") => Promise<void>;
  duplicate: (id: string) => Promise<Rule | null>;
  clearActionError: () => void;
  reset: () => void;
}

const initial = {
  rules: [] as Rule[],
  presets: [] as RuleTemplate[],
  smartCatalog: null as SmartOrganizerCatalog | null,
  status: "idle" as Status,
  loadError: null as UiError | null,
  actionError: null as UiError | null,
};

const sortRules = (items: Rule[]): Rule[] =>
  [...items].sort((a, b) => a.priority - b.priority || (a.createdAt ?? 0) - (b.createdAt ?? 0));

export const useRules = create<RulesState>((set, get) => {
  const upsert = (rule: Rule) =>
    set((s) => ({
      rules: sortRules(s.rules.some((r) => r.id === rule.id) ? s.rules.map((r) => (r.id === rule.id ? rule : r)) : [...s.rules, rule]),
    }));

  const run = async (fn: () => Promise<void>) => {
    set({ actionError: null });
    try {
      await fn();
    } catch (e) {
      set({ actionError: toUiError(e) });
      // Re-sync state from Rust on error (BUG-004 pattern)
      void get().refresh();
    }
  };

  return {
    ...initial,

    load: async () => {
      set({ status: "loading", loadError: null });
      try {
        const [rules, presets, smartCatalog] = await Promise.all([
          rulesService.list(),
          rulesService.presets(),
          rulesService.smartCatalog(),
        ]);
        set({ rules: sortRules(rules), presets, smartCatalog, status: "ready" });
      } catch (e) {
        set({ status: "error", loadError: toUiError(e) });
      }
    },

    refresh: async () => {
      try {
        const rules = await rulesService.list();
        set({ rules: sortRules(rules) });
      } catch {
        // Keep last known good state on background refresh failure.
      }
    },

    create: async (draft) => {
      try {
        const rule = await rulesService.create(draft);
        upsert(rule);
        return { ok: true, rule };
      } catch (e) {
        return { ok: false, error: toUiError(e) };
      }
    },

    createFromPreset: async (presetId) => {
      let created: Rule | null = null;
      await run(async () => {
        created = await rulesService.createFromPreset(presetId);
        upsert(created);
      });
      return created;
    },

    createSmartPreset: async (spec) => {
      let created: Rule | null = null;
      await run(async () => {
        created = await rulesService.createSmartPreset(spec);
        upsert(created);
      });
      return created;
    },

    createSmartPresetResult: async (spec) => {
      try {
        const rule = await rulesService.createSmartPreset(spec);
        upsert(rule);
        return { ok: true, rule };
      } catch (e) {
        return { ok: false, error: toUiError(e) };
      }
    },

    update: async (id, draft) => {
      try {
        const rule = await rulesService.update(id, draft);
        upsert(rule);
        return { ok: true, rule };
      } catch (e) {
        return { ok: false, error: toUiError(e) };
      }
    },

    remove: async (id) => {
      let removed = false;
      await run(async () => {
        await rulesService.remove(id);
        removed = true;
        set((s) => ({ rules: s.rules.filter((r) => r.id !== id) }));
      });
      return removed;
    },

    setEnabled: (id, enabled) =>
      run(async () => {
        const updated = await rulesService.setEnabled(id, enabled);
        upsert(updated);
      }),

    reorder: (orderedIds) =>
      run(async () => {
        const updated = await rulesService.reorder(orderedIds);
        set({ rules: sortRules(updated) });
      }),

    moveRule: async (id, direction) => {
      const current = get().rules;
      const idx = current.findIndex((r) => r.id === id);
      if (idx === -1) return;
      const swapIdx = direction === "up" ? idx - 1 : idx + 1;
      if (swapIdx < 0 || swapIdx >= current.length) return;
      const nextIds = current.map((r) => r.id);
      const a = nextIds[idx];
      const b = nextIds[swapIdx];
      if (!a || !b) return;
      nextIds[idx] = b;
      nextIds[swapIdx] = a;
      await get().reorder(nextIds);
    },

    duplicate: async (id) => {
      let copy: Rule | null = null;
      await run(async () => {
        copy = await rulesService.duplicate(id);
        upsert(copy);
      });
      return copy;
    },

    clearActionError: () => set({ actionError: null }),

    reset: () => set({ ...initial }),
  };
});
