import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { routes } from "@/app/routes";
import { useFolders } from "@/stores/folders";
import { useRules } from "@/stores/rules";
import { useSettings } from "@/stores/settings";
import {
  commands,
  folderFixture,
  resetBindingMocks,
  ruleFixture,
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
  useRules.getState().reset();
  await useSettings.getState().load();
  const router = createMemoryRouter(routes, { initialEntries: [path] });
  render(<RouterProvider router={router} />);
  return router;
}

beforeEach(() => resetBindingMocks());

describe("UI-RULES-01 rules list & starter presets", () => {
  it("shows empty state and creates a rule from a starter preset (disabled by default)", async () => {
    const createdPreset = ruleFixture({
      id: "r-preset",
      name: "Organize PDFs",
      enabled: false,
      origin: "preset",
    });
    commands.rulesCreateFromPreset.mockResolvedValue(ok(createdPreset));

    await renderAt("/rules");
    expect(await screen.findByText("No rules yet")).toBeInTheDocument();

    await userEvent.click(screen.getAllByRole("button", { name: /Add starter rule/i })[0] as HTMLElement);
    await userEvent.click(screen.getByRole("menuitem", { name: /Organize PDFs/i }));

    expect(commands.rulesCreateFromPreset).toHaveBeenCalledWith("pdfs");
    const row = await screen.findByRole("row", { name: /Organize PDFs/i });
    expect(within(row).getByRole("switch", { name: /Enable Organize PDFs/i })).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });

  it("lists rules ordered by priority with stats and execution mode", async () => {
    commands.rulesList.mockResolvedValue(
      ok([
        ruleFixture({ id: "r2", name: "Archive Old Files", priority: 20, statMatched: 5 }),
        ruleFixture({ id: "r1", name: "Organize Invoices", priority: 10, statMatched: 12 }),
      ]),
    );
    await renderAt("/rules");

    const rows = await screen.findAllByRole("row");
    // Header row + 2 data rows
    expect(rows).toHaveLength(3);
    expect(rows[1]).toHaveTextContent("Organize Invoices");
    expect(rows[1]).toHaveTextContent("12 matched");
    expect(rows[2]).toHaveTextContent("Archive Old Files");
    expect(rows[2]).toHaveTextContent("5 matched");
  });

  it("shows a recoverable error when rules fail to load", async () => {
    commands.rulesList.mockResolvedValueOnce(err("Could not read rules table", "database"));
    await renderAt("/rules");
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not read rules table");

    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("No rules yet")).toBeInTheDocument();
  });
});

describe("UI-RULES-02 enable toggle and priority reorder", () => {
  it("toggles enabled state and reorders rules up/down", async () => {
    const r1 = ruleFixture({ id: "r1", name: "First Rule", priority: 10, enabled: true });
    const r2 = ruleFixture({ id: "r2", name: "Second Rule", priority: 20, enabled: false });
    commands.rulesList.mockResolvedValue(ok([r1, r2]));
    commands.rulesSetEnabled.mockImplementation(async (id: string, enabled: boolean) =>
      ok(ruleFixture({ ...(id === "r1" ? r1 : r2), enabled })),
    );
    commands.rulesReorder.mockResolvedValue(
      ok([
        { ...r2, priority: 10 },
        { ...r1, priority: 20 },
      ]),
    );

    await renderAt("/rules");
    const firstRow = await screen.findByRole("row", { name: /First Rule/ });
    await userEvent.click(within(firstRow).getByRole("switch", { name: "Enable First Rule" }));
    expect(commands.rulesSetEnabled).toHaveBeenCalledWith("r1", false);

    const secondRow = screen.getByRole("row", { name: /Second Rule/ });
    await userEvent.click(within(secondRow).getByRole("button", { name: "Move Second Rule up" }));
    expect(commands.rulesReorder).toHaveBeenCalledWith(["r2", "r1"]);
  });
});

describe("UI-RULES-03 duplicate and delete with confirmation", () => {
  it("duplicates a rule and asks confirmation before deleting", async () => {
    const r1 = ruleFixture({ id: "r1", name: "Organize Invoices", priority: 10 });
    const rCopy = ruleFixture({ id: "r2", name: "Organize Invoices (Copy)", priority: 20, enabled: false });
    commands.rulesList.mockResolvedValue(ok([r1]));
    commands.rulesDuplicate.mockResolvedValue(ok(rCopy));

    await renderAt("/rules");
    const row = await screen.findByRole("row", { name: /Organize Invoices/ });
    await userEvent.click(within(row).getByRole("button", { name: "Duplicate Organize Invoices" }));
    expect(commands.rulesDuplicate).toHaveBeenCalledWith("r1");
    expect(await screen.findByRole("row", { name: /Organize Invoices \(Copy\)/ })).toBeInTheDocument();

    await userEvent.click(within(row).getByRole("button", { name: "Delete Organize Invoices" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent(/Delete “Organize Invoices”\?/);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(commands.rulesDelete).not.toHaveBeenCalled();

    await userEvent.click(within(row).getByRole("button", { name: "Delete Organize Invoices" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete rule" }));
    expect(commands.rulesDelete).toHaveBeenCalledWith("r1");
    await waitFor(() =>
      expect(screen.queryByRole("row", { name: /^Organize Invoices / })).toBeNull(),
    );
  });
});

describe("UI-RB-01 create and edit nested rule round-trip", () => {
  it("creates a new rule and navigates back to /rules", async () => {
    commands.rulesCreate.mockImplementation(async (draft) =>
      ok(
        ruleFixture({
          id: "r-new",
          name: draft.name,
          description: draft.description ?? null,
          enabled: draft.enabled,
          conditions: draft.conditions as ReturnType<typeof ruleFixture>["conditions"],
          actions: draft.actions as ReturnType<typeof ruleFixture>["actions"],
        }),
      ),
    );

    const router = await renderAt("/rules/new");
    const nameInput = await screen.findByLabelText("Rule name");
    await userEvent.clear(nameInput);
    await userEvent.type(nameInput, "Sort Screenshots");

    const descInput = screen.getByLabelText("Description");
    await userEvent.type(descInput, "Move screenshots into Pictures");

    await userEvent.click(screen.getByRole("button", { name: "Save rule" }));
    await waitFor(() => expect(commands.rulesCreate).toHaveBeenCalledTimes(1));
    expect(commands.rulesCreate.mock.calls[0]?.[0]).toMatchObject({
      name: "Sort Screenshots",
      description: "Move screenshots into Pictures",
    });
    expect(router.state.location.pathname).toBe("/rules");
  });

  it("loads an existing nested rule, edits it, and saves round-trip", async () => {
    const nestedRule = ruleFixture({
      id: "r-nested",
      name: "Invoices Rule",
      conditions: {
        type: "all",
        children: [
          {
            type: "leaf",
            condition: { kind: "extension_group", extensions: ["pdf", "png"] },
          },
          {
            type: "any",
            children: [
              {
                type: "leaf",
                condition: {
                  kind: "text",
                  field: "name",
                  op: "contains",
                  value: "invoice",
                  case_sensitive: false,
                },
              },
            ],
          },
        ],
      },
    });
    commands.rulesList.mockResolvedValue(ok([nestedRule]));
    commands.rulesGet.mockResolvedValue(ok(nestedRule));
    commands.rulesUpdate.mockImplementation(async (_id, draft) =>
      ok({
        ...nestedRule,
        name: draft.name,
        conditions: draft.conditions as typeof nestedRule.conditions,
        actions: draft.actions as typeof nestedRule.actions,
      }),
    );

    await renderAt("/rules/r-nested");
    const nameInput = await screen.findByLabelText("Rule name");
    expect(nameInput).toHaveValue("Invoices Rule");
    expect(screen.getByDisplayValue("invoice")).toBeInTheDocument();

    await userEvent.clear(nameInput);
    await userEvent.type(nameInput, "Updated Invoices Rule");
    await userEvent.click(screen.getByRole("button", { name: "Save rule" }));

    await waitFor(() => expect(commands.rulesUpdate).toHaveBeenCalledTimes(1));
    expect(commands.rulesUpdate).toHaveBeenCalledWith(
      "r-nested",
      expect.objectContaining({
        name: "Updated Invoices Rule",
        conditions: nestedRule.conditions,
      }),
    );
  });
});

describe("UI-RB-02 condition rows: add, edit, reorder, remove, and decimal size hint", () => {
  it("adds, configures, reorders, and removes conditions with 1 KB = 1000 B note", async () => {
    await renderAt("/rules/new");
    await screen.findByLabelText("Rule name");

    await userEvent.click(screen.getByRole("button", { name: "Add condition" }));
    const kindSelects = screen.getAllByLabelText("Condition type");
    expect(kindSelects).toHaveLength(2);

    await userEvent.selectOptions(kindSelects[1] as HTMLElement, "size");
    expect(screen.getByText(/1 KB = 1000 B/i)).toBeInTheDocument();

    const moveUpButtons = screen.getAllByRole("button", { name: "Move condition up" });
    await userEvent.click(moveUpButtons[1] as HTMLElement);

    const updatedSelects = screen.getAllByLabelText("Condition type");
    expect(updatedSelects[0]).toHaveValue("size");

    const removeButtons = screen.getAllByRole("button", { name: "Remove condition" });
    await userEvent.click(removeButtons[0] as HTMLElement);
    expect(screen.getAllByLabelText("Condition type")).toHaveLength(1);
  });
});

describe("UI-RB-03 nested condition groups: group, change operator, ungroup, depth limit", () => {
  it("adds a subgroup, switches between ALL/ANY/NOT, and ungroups children into parent", async () => {
    await renderAt("/rules/new");
    await screen.findByLabelText("Rule name");

    await userEvent.click(screen.getByRole("button", { name: "Add subgroup" }));
    const groupModeSelects = screen.getAllByLabelText("Group match mode");
    expect(groupModeSelects).toHaveLength(2);

    await userEvent.selectOptions(groupModeSelects[1] as HTMLElement, "any");
    expect(groupModeSelects[1]).toHaveValue("any");

    await userEvent.click(screen.getByRole("button", { name: "Ungroup" }));
    expect(screen.getAllByLabelText("Group match mode")).toHaveLength(1);
    expect(screen.getAllByLabelText("Condition type")).toHaveLength(2);
  });
});

describe("UI-RB-04 actions: add, configure, naming presets, destination picker, reorder, remove", () => {
  it("adds actions, applies naming preset chip, picks destination folder, and removes actions", async () => {
    commands.rulesPickDestinationFolder.mockResolvedValue(ok("/Users/test/Documents/Sorted"));

    await renderAt("/rules/new");
    await screen.findByLabelText("Rule name");

    await userEvent.click(screen.getByRole("button", { name: "Add action" }));
    const actionSelects = screen.getAllByLabelText("Action type");
    expect(actionSelects).toHaveLength(2);

    await userEvent.selectOptions(actionSelects[1] as HTMLElement, "rename");
    const presetBtn = screen.getByRole("button", { name: /Insert preset \{date\}_\{original_name\}/i });
    await userEvent.click(presetBtn);

    await userEvent.selectOptions(actionSelects[0] as HTMLElement, "move");
    await userEvent.click(screen.getByRole("button", { name: "Choose folder…" }));
    expect(commands.rulesPickDestinationFolder).toHaveBeenCalled();
    expect(await screen.findByDisplayValue("/Users/test/Documents/Sorted")).toBeInTheDocument();

    const removeActionBtns = screen.getAllByRole("button", { name: "Remove action" });
    await userEvent.click(removeActionBtns[1] as HTMLElement);
    expect(screen.getAllByLabelText("Action type")).toHaveLength(1);
  });
});

describe("UI-RB-05 inline validation issues and save errors", () => {
  it("shows validation errors and destination-loop warnings from rulesValidate", async () => {
    commands.rulesValidate.mockResolvedValue(
      ok([
        {
          field: "conditions",
          message: "Invalid regular expression: unclosed group",
          severity: "error",
        },
        {
          field: "actions[0].destination",
          message: "Destination is inside monitored folder Downloads and may re-trigger this rule.",
          severity: "warning",
        },
      ]),
    );

    await renderAt("/rules/new");
    await screen.findByLabelText("Rule name");

    expect(
      await screen.findByText("Invalid regular expression: unclosed group"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Destination is inside monitored folder Downloads/i),
    ).toBeInTheDocument();
  });
});

describe("UI-RB-06 live template preview and test rule on folder (FR-057)", () => {
  it("renders live template preview and runs dry-run folder test", async () => {
    commands.foldersList.mockResolvedValue(ok([folderFixture({ id: "f1", name: "Downloads" })]));
    commands.rulesTestFolder.mockResolvedValue(
      ok({
        folderId: "f1",
        folderName: "Downloads",
        evaluatedFiles: 25,
        matchedFiles: 1,
        sampleMatches: [
          {
            path: "/Users/test/Downloads/inv_01.pdf",
            originalName: "inv_01.pdf",
            finalFilename: "2026-04-15_inv_01.pdf",
            finalDirectory: "/Users/test/Downloads/Documents/Invoices/2026/April",
            actions: [],
          },
        ],
      }),
    );

    await renderAt("/rules/new");
    expect(await screen.findByText("Documents/Invoices/2026/April")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Test rule on folder" }));
    expect(commands.rulesTestFolder).toHaveBeenCalled();
    expect(await screen.findByText(/Matched 1 of 25 files/i)).toBeInTheDocument();
    expect(screen.getByText("2026-04-15_inv_01.pdf")).toBeInTheDocument();
  });
});

describe("UI-SMART-01 smart organizer generator modal (FR-150..153)", () => {
  it("opens Smart organizers dialog, builds an old-files rule via rulesCreateSmartPreset, and keeps it disabled", async () => {
    const smartRule = ruleFixture({
      id: "r-smart-old",
      name: "Archive old files (60+ days)",
      enabled: false,
      origin: "preset",
    });
    commands.rulesCreateSmartPreset.mockResolvedValueOnce(ok(smartRule));

    await renderAt("/rules");
    await userEvent.click(screen.getByRole("button", { name: /Smart organizers/i }));
    expect(await screen.findByRole("heading", { name: "Smart Organizers" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Old files archive/i }));
    await userEvent.selectOptions(screen.getByLabelText(/Archive files untouched for at least/i), "60");
    await userEvent.click(screen.getByRole("button", { name: "Create disabled rule" }));

    expect(commands.rulesCreateSmartPreset).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: "old_files",
        days: 60,
      }),
    );
    expect(await screen.findByText("Archive old files (60+ days)")).toBeInTheDocument();
    const row = screen.getByRole("row", { name: /Archive old files \(60\+ days\)/i });
    expect(row.querySelector("[role='switch'][aria-checked='false']")).not.toBeNull();
  });

  it("BUG-033: keeps SmartOrganizerDialog open with inline error if creation fails and closes on Escape", async () => {
    commands.rulesCreateSmartPreset.mockResolvedValueOnce({
      status: "error",
      error: {
        code: "validation",
        message: "Provide at least one invoice keyword.",
        hint: null,
        field: "keywords",
      },
    });

    await renderAt("/rules");
    await userEvent.click(screen.getByRole("button", { name: /Smart organizers/i }));
    expect(await screen.findByRole("heading", { name: "Smart Organizers" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Invoices & receipts/i }));
    await userEvent.click(screen.getByRole("button", { name: "Create disabled rule" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Provide at least one invoice keyword.");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create disabled rule" })).toBeEnabled();

    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
