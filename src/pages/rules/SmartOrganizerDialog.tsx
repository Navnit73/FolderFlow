import { FolderOpen, Wand2 } from "lucide-react";
import { useEffect, useState } from "react";
import { ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { TextField } from "@/components/ui/text-field";
import { cn } from "@/lib/cn";
import { toUiError } from "@/services/result";
import { rulesService, type SmartOrganizerCatalog, type SmartOrganizerSpec } from "@/services/rules";
import type { RuleSaveResult } from "@/stores/rules";
import type { UiError } from "@/types/settings";

type SmartKind = SmartOrganizerSpec["kind"];

const SMART_MODES: { kind: SmartKind; title: string; subtitle: string }[] = [
  {
    kind: "screenshots",
    title: "Screenshots organizer",
    subtitle: "Detects macOS screenshot patterns and files them cleanly by year and month.",
  },
  {
    kind: "invoices",
    title: "Invoices & receipts",
    subtitle: "Matches PDF/document names with billing keywords into an Invoices archive.",
  },
  {
    kind: "download_category",
    title: "Download category",
    subtitle: "Routes a specific file category directly into an organized folder.",
  },
  {
    kind: "old_files",
    title: "Old files archive",
    subtitle: "Archives files untouched for N days (7–365) out of active folders.",
  },
];

interface Props {
  open: boolean;
  catalog: SmartOrganizerCatalog | null;
  onClose: () => void;
  onCreate: (spec: SmartOrganizerSpec) => Promise<RuleSaveResult>;
}

export function SmartOrganizerDialog({ open, catalog, onClose, onCreate }: Props) {
  if (!open) return null;
  return <SmartOrganizerDialogContent catalog={catalog} onClose={onClose} onCreate={onCreate} />;
}

function SmartOrganizerDialogContent({
  catalog,
  onClose,
  onCreate,
}: Omit<Props, "open">) {
  const [kind, setKind] = useState<SmartKind>("screenshots");
  const [extraPatterns, setExtraPatterns] = useState("");
  const [keywords, setKeywords] = useState("invoice, receipt, bill, statement");
  const [categoryId, setCategoryId] = useState(
    catalog?.downloadCategories[0]?.categoryId ?? "documents",
  );
  const [days, setDays] = useState(catalog?.allowedOldFileDays[2] ?? 30);
  const [destination, setDestination] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<UiError | null>(null);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !submitting) onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [submitting, onClose]);

  const pickDest = async () => {
    setFormError(null);
    try {
      const picked = await rulesService.pickDestinationFolder();
      if (picked) setDestination(picked);
    } catch (e) {
      setFormError(toUiError(e));
    }
  };

  const handleCreate = async () => {
    setSubmitting(true);
    setFormError(null);
    let spec: SmartOrganizerSpec;
    switch (kind) {
      case "screenshots":
        spec = {
          kind: "screenshots",
          extra_patterns: extraPatterns
            .split(",")
            .map((s) => s.trim())
            .filter(Boolean),
          destination,
          source_folders: [],
        };
        break;
      case "invoices":
        spec = {
          kind: "invoices",
          keywords: keywords
            .split(",")
            .map((s) => s.trim())
            .filter(Boolean),
          destination,
          source_folders: [],
        };
        break;
      case "download_category":
        spec = {
          kind: "download_category",
          category_id: categoryId,
          destination,
          source_folders: [],
        };
        break;
      case "old_files":
        spec = {
          kind: "old_files",
          days,
          destination,
          source_folders: [],
        };
        break;
    }
    try {
      const res = await onCreate(spec);
      if (res && !res.ok) {
        setFormError(res.error);
        return;
      }
      onClose();
    } catch (e) {
      setFormError(toUiError(e));
    } finally {
      setSubmitting(false);
    }
  };

  const downloadCategories = catalog?.downloadCategories ?? [
    { categoryId: "documents", categoryName: "Documents", defaultSubpath: "Documents" },
  ];
  const ageOptions = catalog?.allowedOldFileDays ?? [7, 15, 30, 60, 90, 180, 365];

  return (
    <div className="fixed inset-0 z-(--ff-z-dialog) flex items-center justify-center bg-overlay p-page">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="smart-org-title"
        className="w-full max-w-xl rounded-xl border border-border bg-popover p-5 shadow-dialog"
      >
        <div className="flex items-center gap-2">
          <Wand2 className="size-4 text-accent-fg" aria-hidden />
          <h2 id="smart-org-title" className="text-base font-semibold text-fg">
            Smart Organizers
          </h2>
        </div>
        <p className="mt-1 text-xs text-fg-muted">
          Creates an ordinary rule you can customize or test in Preview. All smart rules start disabled for safety.
        </p>

        {formError && (
          <div className="mt-3">
            <ErrorState error={formError} onDismiss={() => setFormError(null)} />
          </div>
        )}

        <div className="mt-4 grid gap-2 sm:grid-cols-2">
          {SMART_MODES.map((m) => {
            const active = m.kind === kind;
            return (
              <button
                key={m.kind}
                type="button"
                aria-pressed={active}
                onClick={() => setKind(m.kind)}
                className={cn(
                  "rounded-lg border p-3 text-left transition-colors",
                  active ? "border-border-focus bg-selected" : "border-border bg-surface hover:bg-hover",
                )}
              >
                <p className="text-sm font-medium text-fg">{m.title}</p>
                <p className="mt-0.5 text-xs text-fg-muted">{m.subtitle}</p>
              </button>
            );
          })}
        </div>

        <div className="mt-4 space-y-4 rounded-lg border border-border bg-subtle p-3">
          {kind === "screenshots" && (
            <TextField
              id="smart-screenshot-patterns"
              label="Extra filename patterns (comma-separated, optional)"
              value={extraPatterns}
              onChange={(e) => setExtraPatterns(e.target.value)}
              placeholder="Capture*, CleanShot*"
              hint={`Built-in patterns: ${(catalog?.defaultScreenshotPatterns ?? ["Screenshot*"]).join(", ")}`}
            />
          )}

          {kind === "invoices" && (
            <TextField
              id="smart-invoice-keywords"
              label="Invoice filename keywords (comma-separated)"
              value={keywords}
              onChange={(e) => setKeywords(e.target.value)}
              placeholder="invoice, receipt, bill, statement"
            />
          )}

          {kind === "download_category" && (
            <div className="space-y-1">
              <label htmlFor="smart-download-cat" className="block text-sm font-medium text-fg">
                Target category
              </label>
              <select
                id="smart-download-cat"
                value={categoryId}
                onChange={(e) => setCategoryId(e.target.value)}
                className="h-control w-full rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
              >
                {downloadCategories.map((c) => (
                  <option key={c.categoryId} value={c.categoryId}>
                    {c.categoryName} ({c.defaultSubpath})
                  </option>
                ))}
              </select>
            </div>
          )}

          {kind === "old_files" && (
            <div className="space-y-1">
              <label htmlFor="smart-old-days" className="block text-sm font-medium text-fg">
                Archive files untouched for at least
              </label>
              <select
                id="smart-old-days"
                value={days}
                onChange={(e) => setDays(Number(e.target.value))}
                className="h-control w-full rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
              >
                {ageOptions.map((d) => (
                  <option key={d} value={d}>
                    {d} days
                  </option>
                ))}
              </select>
            </div>
          )}

          <div className="flex items-center justify-between gap-2">
            <div className="min-w-0">
              <p className="text-xs font-medium text-fg">Destination root (optional)</p>
              <p className="truncate text-xs text-fg-muted">
                {destination ?? "Dynamic relative subfolder under user default"}
              </p>
            </div>
            <Button size="sm" variant="secondary" onClick={() => void pickDest()}>
              <FolderOpen aria-hidden />
              Choose folder
            </Button>
          </div>
        </div>

        <div className="mt-5 flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={submitting} onClick={() => void handleCreate()}>
            {submitting ? "Creating…" : "Create disabled rule"}
          </Button>
        </div>
      </div>
    </div>
  );
}

