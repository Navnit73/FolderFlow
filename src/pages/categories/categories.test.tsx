import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { routes } from "@/app/routes";
import { useCategories } from "@/stores/categories";
import { useSettings } from "@/stores/settings";
import { categoryFixture, commands, resetBindingMocks } from "@/test/mockBindings";

vi.mock("@/bindings", () => import("@/test/mockBindings"));

const ok = <T,>(data: T) => ({ status: "ok" as const, data });
const err = (message: string, code = "validation", field: string | null = "name") => ({
  status: "error" as const,
  error: { code, message, hint: null, field },
});

async function renderCategories() {
  useSettings.setState({ settings: null, status: "idle", saving: false, loadError: null, saveError: null });
  useCategories.getState().reset();
  await useSettings.getState().load();
  const router = createMemoryRouter(routes, { initialEntries: ["/categories"] });
  render(<RouterProvider router={router} />);
  return router;
}

beforeEach(() => {
  resetBindingMocks();
});

describe("CategoriesPage (UI-CAT-01)", () => {
  it("lists built-in and custom categories, protects 'other', reorders priority, creates/edits with validation, and deletes custom", async () => {
    const docs = categoryFixture({ id: "documents", name: "Documents", priority: 10, builtin: true, enabled: true });
    const other = categoryFixture({
      id: "other",
      name: "Other",
      description: "Catch-all fallback category",
      icon: "file-question",
      colorToken: "cat-other",
      extensions: [],
      priority: 100,
      builtin: true,
      enabled: true,
    });
    const receipts = categoryFixture({
      id: "custom-receipts",
      name: "Receipts",
      description: "Scanned bills",
      icon: "receipt",
      colorToken: "cat-spreadsheets",
      extensions: ["ofx", "qfx"],
      destinationPath: "/Users/test/Documents/Receipts",
      priority: 5,
      builtin: false,
      enabled: true,
    });

    commands.categoriesList.mockResolvedValue(ok([docs, other]));

    await renderCategories();
    expect(await screen.findByText("Documents")).toBeInTheDocument();
    expect(screen.getByText("Other")).toBeInTheDocument();

    // Built-in fallback 'other' switch cannot be disabled and has an explicit accessible name
    expect(screen.getByRole("switch", { name: "Enable Other category" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Enable Documents category" })).toBeEnabled();
    // Built-in categories do not show Delete/Edit buttons
    expect(screen.queryByRole("button", { name: "Delete Documents" })).not.toBeInTheDocument();

    // Reorder priority updates via categoriesReorder
    commands.categoriesReorder.mockResolvedValueOnce(ok([other, docs]));
    await userEvent.click(screen.getByRole("button", { name: "Move Other up" }));
    expect(commands.categoriesReorder).toHaveBeenCalledWith(["other", "documents"]);

    // Create custom category shows validation error first, then succeeds
    commands.categoriesCreate
      .mockResolvedValueOnce(err("Category name cannot be blank."))
      .mockResolvedValueOnce(ok(receipts));
    commands.categoriesPickDestinationFolder.mockResolvedValueOnce(ok("/Users/test/Documents/Receipts"));

    await userEvent.click(screen.getByRole("button", { name: /New category/i }));
    await userEvent.click(screen.getByRole("button", { name: "Create category" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Category name cannot be blank.");

    await userEvent.type(screen.getByLabelText("Category name"), "Receipts");
    await userEvent.type(screen.getByLabelText(/Extensions/i), ".ofx, .qfx");
    await userEvent.click(screen.getByRole("button", { name: "Color Spreadsheets" }));
    await userEvent.click(screen.getByRole("button", { name: "Choose folder" }));
    await userEvent.click(screen.getByRole("button", { name: "Create category" }));

    await waitFor(() =>
      expect(commands.categoriesCreate).toHaveBeenLastCalledWith(
        expect.objectContaining({
          name: "Receipts",
          extensions: ["ofx", "qfx"],
          colorToken: "cat-spreadsheets",
          destinationPath: "/Users/test/Documents/Receipts",
        }),
      ),
    );
    expect(await screen.findByText("Receipts")).toBeInTheDocument();

    // Edit custom category
    commands.categoriesUpdate.mockResolvedValueOnce(ok({ ...receipts, name: "Tax Receipts" }));
    await userEvent.click(screen.getByRole("button", { name: "Edit Receipts" }));
    const nameInput = screen.getByLabelText("Category name");
    await userEvent.clear(nameInput);
    await userEvent.type(nameInput, "Tax Receipts");
    await userEvent.click(screen.getByRole("button", { name: "Save category" }));
    expect(await screen.findByText("Tax Receipts")).toBeInTheDocument();

    // Delete custom category requires ConfirmDialog
    await userEvent.click(screen.getByRole("button", { name: "Delete Tax Receipts" }));
    expect(screen.getByRole("heading", { name: /Delete custom category\?/i })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(commands.categoriesDelete).toHaveBeenCalledWith("custom-receipts"));
    expect(screen.queryByText("Tax Receipts")).not.toBeInTheDocument();
  });

  it("BUG-033: shows inline error when destination picker fails and closes modal on Escape", async () => {
    const docs = categoryFixture({ id: "documents", name: "Documents", priority: 10, builtin: true, enabled: true });
    commands.categoriesList.mockResolvedValue(ok([docs]));
    commands.categoriesPickDestinationFolder.mockResolvedValueOnce(
      err("Permission denied opening folder picker", "permission"),
    );

    await renderCategories();
    await screen.findByText("Documents");

    await userEvent.click(screen.getByRole("button", { name: /New category/i }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Choose folder" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Permission denied opening folder picker");

    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("UI-CAT-02: displays recoverable ErrorState and reloads on retry when categories fail to load", async () => {
    const docs = categoryFixture({ id: "documents", name: "Documents", priority: 10, builtin: true, enabled: true });
    commands.categoriesList
      .mockResolvedValueOnce(err("SQLite categories table unavailable", "database", null))
      .mockResolvedValue(ok([docs]));

    await renderCategories();
    expect(await screen.findByRole("alert")).toHaveTextContent("SQLite categories table unavailable");

    await userEvent.click(screen.getByRole("button", { name: /Try again/i }));
    expect(await screen.findByText("Documents")).toBeInTheDocument();
  });
});
