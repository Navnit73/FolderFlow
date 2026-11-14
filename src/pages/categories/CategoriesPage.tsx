import { ArrowDown, ArrowUp, FolderOpen, Pencil, Plus, Tags, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState, ErrorState, LoadingState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Switch } from "@/components/ui/switch";
import { TextField } from "@/components/ui/text-field";
import { CATEGORY_ICON_OPTIONS, categoryIcon } from "@/lib/categoryIcon";
import { CATEGORY_COLOR_TOKENS, categorySwatch } from "@/lib/categoryStyle";
import { cn } from "@/lib/cn";
import { categoriesService, type Category, type CategoryInput } from "@/services/categories";
import { toUiError } from "@/services/result";
import { useCategories } from "@/stores/categories";
import type { UiError } from "@/types/settings";

interface FormDraft {
  name: string;
  description: string;
  extensionsRaw: string;
  icon: string;
  colorToken: string;
  destinationPath: string | null;
  priority: number;
  enabled: boolean;
}

const DEFAULT_DRAFT: FormDraft = {
  name: "",
  description: "",
  extensionsRaw: "",
  icon: "tag",
  colorToken: "cat-documents",
  destinationPath: null,
  priority: 50,
  enabled: true,
};

function draftFromCategory(c: Category): FormDraft {
  return {
    name: c.name,
    description: c.description ?? "",
    extensionsRaw: c.extensions.join(", "),
    icon: c.icon,
    colorToken: c.colorToken,
    destinationPath: c.destinationPath,
    priority: c.priority,
    enabled: c.enabled,
  };
}

export function CategoriesPage() {
  const {
    categories,
    status,
    loadError,
    actionError,
    load,
    create,
    update,
    remove,
    setEnabled,
    moveCategory,
    clearActionError,
  } = useCategories();

  const [editing, setEditing] = useState<Category | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const [draft, setDraft] = useState<FormDraft>(DEFAULT_DRAFT);
  const [formError, setFormError] = useState<UiError | null>(null);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState<Category | null>(null);

  useEffect(() => {
    if (status === "idle") void load();
  }, [status, load]);

  const openCreate = () => {
    setEditing(null);
    setDraft(DEFAULT_DRAFT);
    setFormError(null);
    setModalOpen(true);
  };

  const openEdit = (cat: Category) => {
    setEditing(cat);
    setDraft(draftFromCategory(cat));
    setFormError(null);
    setModalOpen(true);
  };

  useEffect(() => {
    if (!modalOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !saving) setModalOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [modalOpen, saving]);

  const handlePickDest = async () => {
    setFormError(null);
    try {
      const picked = await categoriesService.pickDestinationFolder();
      if (picked) {
        setDraft((d) => ({ ...d, destinationPath: picked }));
      }
    } catch (e) {
      setFormError(toUiError(e));
    }
  };

  const handleSave = async () => {
    setSaving(true);
    setFormError(null);
    const input: CategoryInput = {
      name: draft.name,
      description: draft.description.trim() || null,
      icon: draft.icon,
      colorToken: draft.colorToken,
      extensions: draft.extensionsRaw
        .split(",")
        .map((s) => s.trim().replace(/^\.+/, "").toLowerCase())
        .filter(Boolean),
      destinationPath: draft.destinationPath,
      priority: Number.isFinite(draft.priority) ? draft.priority : 50,
      enabled: draft.enabled,
    };

    const res = editing ? await update(editing.id, input) : await create(input);
    setSaving(false);
    if (!res.ok) {
      setFormError(res.error);
      return;
    }
    setModalOpen(false);
  };

  const headerActions = (
    <Button variant="primary" onClick={openCreate}>
      <Plus aria-hidden />
      New category
    </Button>
  );

  let body;
  if (status === "error" && loadError) {
    body = <ErrorState error={loadError} onRetry={() => void load()} />;
  } else if (status !== "ready") {
    body = <LoadingState label="Loading categories…" />;
  } else if (categories.length === 0) {
    body = (
      <EmptyState
        icon={Tags}
        title="No categories available"
        description="Create a custom category to organize specialized file types."
        action={
          <Button variant="primary" onClick={openCreate}>
            <Plus aria-hidden />
            New category
          </Button>
        }
      />
    );
  } else {
    body = (
      <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-surface">
        {categories.map((cat, idx) => {
          const Icon = categoryIcon(cat.icon);
          const isOther = cat.id === "other";
          return (
            <li key={cat.id} className="flex flex-wrap items-center justify-between gap-4 px-4 py-3">
              <div className="flex min-w-0 flex-1 items-start gap-3">
                <span className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg border border-border bg-subtle">
                  <Icon className="size-4 text-fg" aria-hidden />
                </span>
                <div className="min-w-0 space-y-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className={cn("size-2.5 rounded-full", categorySwatch(cat.colorToken))} />
                    <span className="text-sm font-semibold text-fg">{cat.name}</span>
                    <span className="rounded-full bg-subtle px-2 py-0.5 text-2xs font-medium text-fg-muted">
                      {cat.builtin ? "Built-in" : "Custom"}
                    </span>
                    <span className="text-xs text-fg-subtle">Priority {cat.priority}</span>
                  </div>
                  {cat.description && <p className="text-xs text-fg-muted">{cat.description}</p>}
                  <div className="flex flex-wrap items-center gap-1.5">
                    {cat.extensions.slice(0, 10).map((ext) => (
                      <span key={ext} className="rounded-sm bg-muted px-1.5 py-0.5 text-2xs font-mono text-fg-muted">
                        .{ext}
                      </span>
                    ))}
                    {cat.extensions.length > 10 && (
                      <span className="text-2xs text-fg-subtle">+{cat.extensions.length - 10} more</span>
                    )}
                  </div>
                  {cat.destinationPath && (
                    <p className="text-xs text-fg-subtle">Destination: {cat.destinationPath}</p>
                  )}
                </div>
              </div>

              <div className="flex shrink-0 items-center gap-2">
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={`Move ${cat.name} up`}
                  disabled={idx === 0}
                  onClick={() => void moveCategory(cat.id, "up")}
                >
                  <ArrowUp aria-hidden />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={`Move ${cat.name} down`}
                  disabled={idx === categories.length - 1}
                  onClick={() => void moveCategory(cat.id, "down")}
                >
                  <ArrowDown aria-hidden />
                </Button>
                {!cat.builtin && (
                  <>
                    <Button
                      size="sm"
                      variant="secondary"
                      aria-label={`Edit ${cat.name}`}
                      onClick={() => openEdit(cat)}
                    >
                      <Pencil aria-hidden />
                      Edit
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Delete ${cat.name}`}
                      onClick={() => setDeleting(cat)}
                    >
                      <Trash2 aria-hidden />
                    </Button>
                  </>
                )}
                <Switch
                  id={`cat-enable-${cat.id}`}
                  checked={cat.enabled}
                  disabled={isOther}
                  onChange={(checked) => void setEnabled(cat.id, checked)}
                />
              </div>
            </li>
          );
        })}
      </ul>
    );
  }

  return (
    <>
      <PageHeader
        title="Categories"
        description="Manage file type categories, matching priority order, custom extensions, and default destinations. Changes update the live classifier immediately."
        actions={headerActions}
      />
      <div className="min-h-0 flex-1 overflow-y-auto p-page">
        <div className="mx-auto max-w-content space-y-4">
          {actionError && <ErrorState error={actionError} onDismiss={clearActionError} />}
          {body}
        </div>
      </div>

      {modalOpen && (
        <div className="fixed inset-0 z-(--ff-z-dialog) flex items-center justify-center bg-overlay p-page">
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="cat-dialog-title"
            className="w-full max-w-lg rounded-xl border border-border bg-popover p-5 shadow-dialog"
          >
            <h2 id="cat-dialog-title" className="text-base font-semibold text-fg">
              {editing ? `Edit category: ${editing.name}` : "New custom category"}
            </h2>
            <div className="mt-4 space-y-4">
              {formError && <ErrorState error={formError} onDismiss={() => setFormError(null)} />}

              <TextField
                id="cat-name"
                label="Category name"
                value={draft.name}
                onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))}
                placeholder="e.g. Receipts & Bills"
              />

              <TextField
                id="cat-extensions"
                label="Extensions (comma-separated)"
                value={draft.extensionsRaw}
                onChange={(e) => setDraft((d) => ({ ...d, extensionsRaw: e.target.value }))}
                placeholder="pdf, ofx, qfx"
                hint="Normalized automatically (leading dots and spaces removed)."
              />

              <TextField
                id="cat-desc"
                label="Description (optional)"
                value={draft.description}
                onChange={(e) => setDraft((d) => ({ ...d, description: e.target.value }))}
              />

              <div className="space-y-1.5">
                <p className="text-sm font-medium text-fg">Color token</p>
                <div className="flex flex-wrap gap-2">
                  {CATEGORY_COLOR_TOKENS.map(({ token, label }) => {
                    const selected = draft.colorToken === token;
                    return (
                      <button
                        key={token}
                        type="button"
                        aria-label={`Color ${label}`}
                        aria-pressed={selected}
                        onClick={() => setDraft((d) => ({ ...d, colorToken: token }))}
                        className={cn(
                          "flex items-center gap-1.5 rounded-full border px-2 py-1 text-xs",
                          selected ? "border-border-focus bg-selected text-fg" : "border-border text-fg-muted",
                        )}
                      >
                        <span className={cn("size-3 rounded-full", categorySwatch(token))} />
                        {label}
                      </button>
                    );
                  })}
                </div>
              </div>

              <div className="space-y-1.5">
                <p className="text-sm font-medium text-fg">Icon</p>
                <div className="grid grid-cols-8 gap-1.5">
                  {CATEGORY_ICON_OPTIONS.map(({ id, label, Icon }) => {
                    const selected = draft.icon === id;
                    return (
                      <button
                        key={id}
                        type="button"
                        aria-label={`Icon ${label}`}
                        aria-pressed={selected}
                        onClick={() => setDraft((d) => ({ ...d, icon: id }))}
                        className={cn(
                          "flex h-control items-center justify-center rounded-md border",
                          selected ? "border-border-focus bg-selected text-fg" : "border-border text-fg-muted hover:bg-hover",
                        )}
                      >
                        <Icon className="size-4" aria-hidden />
                      </button>
                    );
                  })}
                </div>
              </div>

              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <p className="text-sm font-medium text-fg">Default destination</p>
                  <p className="truncate text-xs text-fg-muted">
                    {draft.destinationPath ?? "None selected"}
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  {draft.destinationPath && (
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => setDraft((d) => ({ ...d, destinationPath: null }))}
                    >
                      Clear
                    </Button>
                  )}
                  <Button size="sm" variant="secondary" onClick={() => void handlePickDest()}>
                    <FolderOpen aria-hidden />
                    Choose folder
                  </Button>
                </div>
              </div>
            </div>

            <div className="mt-5 flex justify-end gap-2">
              <Button variant="secondary" onClick={() => setModalOpen(false)}>
                Cancel
              </Button>
              <Button variant="primary" disabled={saving} onClick={() => void handleSave()}>
                {saving ? "Saving…" : editing ? "Save category" : "Create category"}
              </Button>
            </div>
          </div>
        </div>
      )}

      <ConfirmDialog
        open={deleting !== null}
        title="Delete custom category?"
        confirmLabel="Delete"
        danger
        onConfirm={async () => {
          if (deleting) {
            await remove(deleting.id);
            setDeleting(null);
          }
        }}
        onCancel={() => setDeleting(null)}
      >
        <p>
          Delete custom category <span className="font-medium text-fg">{deleting?.name}</span>? Existing
          files already classified remain intact on disk.
        </p>
      </ConfirmDialog>
    </>
  );
}
