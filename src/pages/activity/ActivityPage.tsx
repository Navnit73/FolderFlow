import {
  AlertTriangle,
  Download,
  ExternalLink,
  History,
  Layers,
  RotateCcw,
  Trash2,
  Undo2,
} from "lucide-react";
import { useEffect, useState } from "react";
import { PageHeader } from "@/components/PageHeader";
import { PathLabel } from "@/components/PathLabel";
import { EmptyState, ErrorState, LoadingState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { formatBytes, formatRelative } from "@/lib/format";
import type { OpKind, OperationView, OpStatus } from "@/services/activity";
import { useActivity } from "@/stores/activity";

const STATUS_OPTIONS: Array<{ value: "" | OpStatus; label: string }> = [
  { value: "", label: "All statuses" },
  { value: "COMPLETED", label: "Completed" },
  { value: "FAILED", label: "Failed" },
  { value: "ROLLED_BACK", label: "Rolled back" },
  { value: "IN_PROGRESS", label: "In progress" },
  { value: "PENDING", label: "Pending" },
];

const TYPE_OPTIONS: Array<{ value: "" | OpKind; label: string }> = [
  { value: "", label: "All types" },
  { value: "move", label: "Move" },
  { value: "copy", label: "Copy" },
  { value: "rename", label: "Rename" },
  { value: "undo_move", label: "Undo move" },
  { value: "undo_copy", label: "Undo copy" },
  { value: "undo_rename", label: "Undo rename" },
];

function statusToneClass(status: string): string {
  switch (status) {
    case "COMPLETED":
      return "bg-success-subtle text-success";
    case "FAILED":
      return "bg-danger-subtle text-danger";
    case "ROLLED_BACK":
    case "SKIPPED":
      return "bg-warning-subtle text-warning";
    default:
      return "bg-info-subtle text-info";
  }
}

function canUndoOperation(op: OperationView): boolean {
  return op.status === "COMPLETED" && op.reversible && !op.reversedBy;
}

function canRetryOperation(op: OperationView): boolean {
  return op.status === "FAILED" || op.status === "ROLLED_BACK";
}

export function ActivityPage() {
  const {
    items,
    total,
    page,
    filter,
    reviews,
    undoConflicts,
    lastUndoSummary,
    status,
    loadError,
    actionError,
    load,
    setFilter,
    resetFilter,
    setPage,
    undo,
    retry,
    exportCsv,
    clearHistory,
    revealInFinder,
    resolveReview,
    dismissUndoConflicts,
    clearActionError,
  } = useActivity();

  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [confirmClearOpen, setConfirmClearOpen] = useState(false);
  const [csvNotice, setCsvNotice] = useState<string | null>(null);

  useEffect(() => {
    if (status === "idle") {
      void load();
    }
  }, [status, load]);

  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const handleUndoLast = async () => {
    await undo({ kind: "last", auto_rename_conflicts: false });
  };

  const handleUndoSelected = async () => {
    const ids = Array.from(selectedIds);
    if (ids.length === 0) return;
    await undo({ kind: "ops", op_ids: ids, auto_rename_conflicts: false });
    setSelectedIds(new Set());
  };

  const handleExportCsv = async () => {
    const csv = await exportCsv();
    if (csv !== null) {
      setCsvNotice(`CSV exported (${csv.split("\n").filter(Boolean).length} lines)`);
      if (
        typeof URL !== "undefined" &&
        typeof URL.createObjectURL === "function" &&
        typeof navigator !== "undefined" &&
        !navigator.userAgent.includes("jsdom")
      ) {
        try {
          const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
          const url = URL.createObjectURL(blob);
          const a = document.createElement("a");
          a.href = url;
          a.download = "folderflow-activity.csv";
          a.click();
          URL.revokeObjectURL(url);
        } catch {
          // Ignore browser download fallback errors in headless environments.
        }
      }
    }
  };

  const hasFilters = Boolean(
    filter.status || filter.operationType || filter.search || filter.ruleId,
  );

  const headerActions = (
    <>
      {selectedIds.size > 0 && (
        <Button variant="secondary" onClick={() => void handleUndoSelected()}>
          <Undo2 aria-hidden />
          {`Undo Selected (${selectedIds.size})`}
        </Button>
      )}
      <Button variant="secondary" onClick={() => void handleUndoLast()}>
        <Undo2 aria-hidden />
        Undo Last
      </Button>
      <Button variant="secondary" onClick={() => void handleExportCsv()}>
        <Download aria-hidden />
        Export CSV
      </Button>
      <Button variant="ghost" onClick={() => setConfirmClearOpen(true)}>
        <Trash2 aria-hidden />
        Clear History
      </Button>
    </>
  );

  let body;
  if (status === "error" && loadError) {
    body = <ErrorState error={loadError} onRetry={() => void load()} />;
  } else if (status === "idle" || status === "loading") {
    body = <LoadingState />;
  } else if (items.length === 0 && !hasFilters) {
    body = (
      <EmptyState
        icon={History}
        title="No activity yet"
        description="Every move, copy, rename, and undo is recorded here in a write-ahead ledger so you can inspect or reverse it at any time."
      />
    );
  } else if (items.length === 0) {
    body = (
      <EmptyState
        icon={History}
        title="No matching operations"
        description="Try adjusting your search or filter criteria."
        action={
          <Button variant="secondary" onClick={() => void resetFilter()}>
            Clear filters
          </Button>
        }
      />
    );
  } else {
    body = (
      <div className="space-y-3">
        <table className="w-full table-fixed border-collapse text-sm">
          <thead>
            <tr className="h-row border-b border-border text-left text-xs text-fg-muted">
              <th className="w-8 font-medium">
                <span className="sr-only">Select</span>
              </th>
              <th className="w-2/5 font-medium">File &amp; paths</th>
              <th className="w-24 font-medium">Type</th>
              <th className="w-32 font-medium">Rule</th>
              <th className="w-28 font-medium">Status</th>
              <th className="w-28 font-medium">When</th>
              <th className="sr-only">Actions</th>
            </tr>
          </thead>
          <tbody>
            {items.map((op) => {
              const undoable = canUndoOperation(op);
              const retryable = canRetryOperation(op);
              const batchId = op.batchId;
              return (
                <tr
                  key={op.id}
                  className="border-b border-border last:border-b-0 hover:bg-hover"
                >
                  <td className="py-2 pr-2 align-top">
                    <input
                      type="checkbox"
                      aria-label={`Select operation ${op.id}`}
                      checked={selectedIds.has(op.id)}
                      onChange={() => toggleSelect(op.id)}
                    />
                  </td>
                  <td className="py-2 pr-4 align-top">
                    <div className="flex items-center gap-2">
                      <span className="truncate font-medium text-fg">
                        {op.originalName}
                      </span>
                      {op.newName !== op.originalName && (
                        <span className="truncate text-xs text-fg-muted">
                          {`→ ${op.newName}`}
                        </span>
                      )}
                      <span className="text-2xs text-fg-subtle">
                        {formatBytes(op.size)}
                      </span>
                    </div>
                    <div className="mt-0.5 space-y-0.5 text-xs text-fg-muted">
                      <PathLabel path={op.sourcePath} />
                      <PathLabel path={op.destinationPath} />
                    </div>
                    {op.errorMessage && (
                      <p className="mt-1 text-xs text-danger">{op.errorMessage}</p>
                    )}
                  </td>
                  <td className="py-2 pr-2 align-top">
                    <span className="rounded-sm bg-muted px-1.5 py-0.5 font-mono text-2xs text-fg-muted">
                      {op.operationType}
                    </span>
                  </td>
                  <td className="py-2 pr-2 align-top text-xs text-fg-muted">
                    {op.ruleName ?? "Manual"}
                  </td>
                  <td className="py-2 pr-2 align-top">
                    <div className="flex flex-wrap items-center gap-1">
                      <span
                        className={`rounded-sm px-1.5 py-0.5 text-2xs font-medium ${statusToneClass(
                          op.status,
                        )}`}
                      >
                        {op.status}
                      </span>
                      {op.reversedBy && (
                        <span className="rounded-sm bg-muted px-1.5 py-0.5 text-2xs text-fg-subtle">
                          Undone
                        </span>
                      )}
                    </div>
                  </td>
                  <td className="py-2 pr-2 align-top text-xs text-fg-muted">
                    {formatRelative(op.startedAt)}
                  </td>
                  <td className="py-2 align-top">
                    <div className="flex justify-end gap-0.5">
                      <Button
                        size="icon"
                        variant="ghost"
                        aria-label={`Reveal source in Finder for ${op.id}`}
                        title="Reveal source in Finder"
                        onClick={() => void revealInFinder(op.id, "source")}
                      >
                        <ExternalLink />
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        aria-label={`Reveal destination in Finder for ${op.id}`}
                        title="Reveal destination in Finder"
                        onClick={() => void revealInFinder(op.id, "destination")}
                      >
                        <ExternalLink />
                      </Button>
                      {undoable && (
                        <Button
                          size="icon"
                          variant="ghost"
                          aria-label={`Undo operation ${op.id}`}
                          title="Undo operation"
                          onClick={() =>
                            void undo({
                              kind: "ops",
                              op_ids: [op.id],
                              auto_rename_conflicts: false,
                            })
                          }
                        >
                          <Undo2 />
                        </Button>
                      )}
                      {undoable && batchId && (
                        <Button
                          size="icon"
                          variant="ghost"
                          aria-label={`Undo batch for ${op.id}`}
                          title="Undo entire batch"
                          onClick={() =>
                            void undo({
                              kind: "batch",
                              batch_id: batchId,
                              restore_structure: true,
                              auto_rename_conflicts: false,
                            })
                          }
                        >
                          <Layers />
                        </Button>
                      )}
                      {retryable && (
                        <Button
                          size="icon"
                          variant="ghost"
                          aria-label={`Retry operation ${op.id}`}
                          title="Retry operation"
                          onClick={() => void retry(op.id)}
                        >
                          <RotateCcw />
                        </Button>
                      )}
                    </div>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>

        {total > page.limit && (
          <div className="flex items-center justify-between border-t border-border pt-3 text-xs text-fg-muted">
            <span>{`Showing ${page.offset + 1}–${Math.min(page.offset + items.length, total)} of ${total}`}</span>
            <div className="flex items-center gap-2">
              <Button
                variant="secondary"
                disabled={page.offset === 0}
                onClick={() => void setPage(page.offset - page.limit)}
              >
                Previous
              </Button>
              <Button
                variant="secondary"
                disabled={page.offset + page.limit >= total}
                onClick={() => void setPage(page.offset + page.limit)}
              >
                Next
              </Button>
            </div>
          </div>
        )}
      </div>
    );
  }

  return (
    <>
      <PageHeader
        title="Activity"
        description="Write-ahead operation ledger with one-click undo, occupied-path protection, and review queue."
        actions={headerActions}
      />
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-page">
        {/* Filter bar */}
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="search"
            data-ff-search="true"
            aria-label="Search activity"
            placeholder="Search file names or paths…"
            value={filter.search ?? ""}
            onChange={(e) =>
              void setFilter({ search: e.target.value || undefined })
            }
            className="h-control flex-1 rounded-md border border-input bg-surface px-2.5 text-sm text-fg placeholder:text-fg-subtle"
          />
          <select
            aria-label="Filter by status"
            value={filter.status ?? ""}
            onChange={(e) => {
              const val = e.target.value as "" | OpStatus;
              void setFilter({ status: val ? val : undefined });
            }}
            className="h-control rounded-md border border-input bg-surface px-2 text-sm text-fg"
          >
            {STATUS_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
          <select
            aria-label="Filter by type"
            value={filter.operationType ?? ""}
            onChange={(e) => {
              const val = e.target.value as "" | OpKind;
              void setFilter({ operationType: val ? val : undefined });
            }}
            className="h-control rounded-md border border-input bg-surface px-2 text-sm text-fg"
          >
            {TYPE_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
          {hasFilters && (
            <Button variant="ghost" onClick={() => void resetFilter()}>
              Reset
            </Button>
          )}
        </div>

        {actionError && (
          <ErrorState error={actionError} onDismiss={clearActionError} />
        )}

        {csvNotice && (
          <div
            role="status"
            className="flex items-center justify-between rounded-lg border border-border bg-info-subtle px-3 py-2 text-xs text-fg"
          >
            <span>{csvNotice}</span>
            <Button variant="ghost" onClick={() => setCsvNotice(null)}>
              Dismiss
            </Button>
          </div>
        )}

        {lastUndoSummary && (
          <div
            role="status"
            className="rounded-lg border border-border bg-success-subtle px-3 py-2 text-xs text-fg"
          >
            {lastUndoSummary}
          </div>
        )}

        {undoConflicts.length > 0 && (
          <div
            role="alert"
            className="space-y-2 rounded-lg border border-warning bg-warning-subtle p-3 text-sm text-fg"
          >
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2 font-medium">
                <AlertTriangle className="size-4 text-warning" aria-hidden />
                <span>Undo blocked by occupied original path</span>
              </div>
              <div className="flex items-center gap-2">
                <Button
                  variant="primary"
                  onClick={() =>
                    void undo({
                      kind: "ops",
                      op_ids: undoConflicts.map((c) => c.opId),
                      auto_rename_conflicts: true,
                    })
                  }
                >
                  Restore with Auto-Rename
                </Button>
                <Button variant="ghost" onClick={dismissUndoConflicts}>
                  Dismiss
                </Button>
              </div>
            </div>
            <ul className="space-y-1 text-xs text-fg-muted">
              {undoConflicts.map((c) => (
                <li key={c.opId}>
                  <span className="font-mono text-fg">{c.originalPath}</span>:{" "}
                  {c.reason}
                </li>
              ))}
            </ul>
          </div>
        )}

        {reviews.length > 0 && (
          <section
            aria-label="Needs Review"
            className="space-y-2 rounded-lg border border-warning bg-warning-subtle p-3"
          >
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold text-fg">
                {`Needs Review (${reviews.length})`}
              </h2>
              <span className="text-xs text-fg-muted">
                Destination conflicts waiting for your decision
              </span>
            </div>
            <div className="space-y-2">
              {reviews.map((rev) => (
                <div
                  key={rev.id}
                  className="flex items-center justify-between gap-3 rounded-md border border-border bg-surface p-2.5 text-sm"
                >
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium text-fg">
                      {rev.proposed?.originalName ?? rev.path}
                    </div>
                    {rev.proposed && (
                      <div className="mt-0.5 space-y-0.5 text-xs text-fg-muted">
                        <PathLabel path={rev.proposed.sourcePath} />
                        <PathLabel path={rev.proposed.destinationPath} />
                      </div>
                    )}
                  </div>
                  <div className="flex items-center gap-1.5">
                    <Button
                      variant="primary"
                      aria-label={`Keep Both for review ${rev.id}`}
                      onClick={() => void resolveReview(rev.id, "keep_both")}
                    >
                      Keep Both
                    </Button>
                    <Button
                      variant="secondary"
                      aria-label={`Skip for review ${rev.id}`}
                      onClick={() => void resolveReview(rev.id, "skip")}
                    >
                      Skip
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          </section>
        )}

        {body}
      </div>

      <ConfirmDialog
        open={confirmClearOpen}
        title="Clear Activity History"
        confirmLabel="Confirm Clear History"
        danger
        onCancel={() => setConfirmClearOpen(false)}
        onConfirm={() => {
          setConfirmClearOpen(false);
          void clearHistory(null);
        }}
      >
        This clears completed and failed history records from the activity log. Active operations are never removed, and your files on disk are untouched.
      </ConfirmDialog>
    </>
  );
}
