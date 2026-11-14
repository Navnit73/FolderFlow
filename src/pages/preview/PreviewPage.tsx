import { AlertTriangle, CheckCircle2, Eye, Play, RefreshCw, ShieldAlert, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router";
import { commands, type MonitoredFolder_Serialize, type Rule_Serialize } from "@/bindings";
import { PageHeader } from "@/components/PageHeader";
import { PathLabel } from "@/components/PathLabel";
import { EmptyState, ErrorState, LoadingState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { cn } from "@/lib/cn";
import { formatBytes, plural } from "@/lib/format";
import type { ConflictPolicy, PlanItem, PlanItemStatus } from "@/services/preview";
import { usePreview } from "@/stores/preview";

const STATUS_BADGE: Record<PlanItemStatus, { label: string; className: string }> = {
  ready: { label: "Ready", className: "bg-success-subtle text-success" },
  conflict: { label: "Conflict", className: "bg-warning-subtle text-warning" },
  needs_review: { label: "Needs review", className: "bg-info-subtle text-info" },
  blocked: { label: "Blocked", className: "bg-danger-subtle text-danger" },
};

const POLICY_OPTIONS: { value: ConflictPolicy; label: string }[] = [
  { value: "auto_rename", label: "Auto-rename collisions" },
  { value: "keep_both", label: "Keep both" },
  { value: "skip", label: "Skip existing" },
  { value: "ask", label: "Send collisions to Review" },
];

export function PreviewPage() {
  const {
    summary,
    items,
    total,
    scope,
    filter,
    conflictPolicy,
    selectedIds,
    executing,
    lastReport,
    status,
    loadError,
    actionError,
    generate,
    setFilter,
    resetFilter,
    setConflictPolicy,
    toggleItem,
    selectAllSelectable,
    deselectAll,
    executeSelected,
    discard,
    clearActionError,
  } = usePreview();

  const [folders, setFolders] = useState<MonitoredFolder_Serialize[]>([]);
  const [rules, setRules] = useState<Rule_Serialize[]>([]);
  const [confirmOpen, setConfirmOpen] = useState(false);

  useEffect(() => {
    void generate();
    void commands.foldersList().then((res) => {
      if (res.status === "ok") setFolders(res.data);
    });
    void commands.rulesList().then((res) => {
      if (res.status === "ok") setRules(res.data);
    });
  }, [generate]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
        if (selectedIds.length > 0 && !executing && !confirmOpen) {
          e.preventDefault();
          setConfirmOpen(true);
        }
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [selectedIds.length, executing, confirmOpen]);

  const selectableItems = items.filter((item) => item.status !== "blocked");
  const allSelectableChecked =
    selectableItems.length > 0 &&
    selectableItems.every((item) => selectedIds.includes(item.id));

  const selectedSet = new Set(selectedIds);

  const handleConfirmExecute = async () => {
    setConfirmOpen(false);
    await executeSelected();
  };

  let body;
  if (status === "error" && loadError) {
    body = <ErrorState error={loadError} onRetry={() => void generate()} />;
  } else if (status === "loading" && !summary) {
    body = <LoadingState label="Evaluating rules (dry run)…" />;
  } else if (!summary) {
    body = (
      <EmptyState
        icon={Eye}
        title="No active preview plan"
        description="Run a dry-run preview to see every file FolderFlow would move, copy, or rename before any changes happen."
        action={
          <Button variant="primary" onClick={() => void generate()}>
            <RefreshCw aria-hidden />
            Generate preview
          </Button>
        }
      />
    );
  } else {
    body = (
      <div className="space-y-4">
        {/* Summary metrics (FR-083) */}
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <div className="rounded-lg border border-border bg-subtle p-3">
            <p className="text-xs text-fg-muted">Files evaluated</p>
            <p className="mt-1 text-2xl font-semibold text-fg">
              {summary.filesEvaluated.toLocaleString()}
            </p>
          </div>
          <div className="rounded-lg border border-border bg-subtle p-3">
            <p className="text-xs text-fg-muted">Matched actions</p>
            <p className="mt-1 text-2xl font-semibold text-fg">
              {summary.filesMatched.toLocaleString()}
            </p>
          </div>
          <div className="rounded-lg border border-border bg-subtle p-3">
            <p className="text-xs text-fg-muted">Conflicts / blocked</p>
            <p
              className={cn(
                "mt-1 text-2xl font-semibold",
                summary.filesWithConflicts > 0 ? "text-warning" : "text-fg",
              )}
            >
              {summary.filesWithConflicts.toLocaleString()}
            </p>
          </div>
          <div className="rounded-lg border border-border bg-subtle p-3">
            <p className="text-xs text-fg-muted">Data affected</p>
            <p className="mt-1 text-2xl font-semibold text-fg">
              {formatBytes(summary.totalBytesAffected)}
            </p>
            {(summary.crossVolumeBytes ?? 0) > 0 && (
              <p className="mt-0.5 text-2xs text-fg-muted">
                {formatBytes(summary.crossVolumeBytes)} cross-volume
              </p>
            )}
          </div>
        </div>

        {(summary.byCategory.length > 0 || summary.byRule.length > 0) && (
          <div className="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-surface p-3 text-xs">
            <span className="font-medium text-fg-muted">Breakdown:</span>
            {summary.byCategory.map((c) => (
              <span
                key={c.category}
                className="inline-flex items-center gap-1 rounded-sm bg-muted px-2 py-0.5 text-fg"
              >
                <span className="capitalize">{c.category}</span>
                <span className="text-fg-muted">
                  ({c.count} · {formatBytes(c.bytes)})
                </span>
              </span>
            ))}
            {summary.byRule.map((r) => (
              <span
                key={r.ruleId}
                className="inline-flex items-center gap-1 rounded-sm bg-accent-subtle px-2 py-0.5 text-accent-fg"
              >
                <span>{r.ruleName}</span>
                <span>({r.count})</span>
              </span>
            ))}
          </div>
        )}

        {/* Scope & filter toolbar */}
        <div className="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-subtle p-2">
          <select
            aria-label="Filter scope by folder"
            value={scope.folderId ?? ""}
            onChange={(e) => void generate({ folderId: e.target.value || null })}
            className="h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          >
            <option value="">All monitored folders</option>
            {folders.map((f) => (
              <option key={f.id} value={f.id}>
                {f.name}
              </option>
            ))}
          </select>

          <select
            aria-label="Filter scope by rule"
            value={scope.ruleId ?? ""}
            onChange={(e) => void generate({ ruleId: e.target.value || null })}
            className="h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          >
            <option value="">All enabled rules</option>
            {rules.map((r) => (
              <option key={r.id} value={r.id}>
                {r.name}
              </option>
            ))}
          </select>

          <input
            type="search"
            data-ff-search="true"
            aria-label="Search planned files"
            placeholder="Search filename, rule, or destination…"
            value={filter.search ?? ""}
            onChange={(e) => void setFilter({ search: e.target.value || null })}
            className="h-control min-w-48 flex-1 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg placeholder:text-fg-subtle"
          />

          <select
            aria-label="Filter by category"
            value={filter.category ?? ""}
            onChange={(e) => void setFilter({ category: e.target.value || null })}
            className="h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          >
            <option value="">All categories</option>
            {summary.byCategory.map((c) => (
              <option key={c.category} value={c.category}>
                {c.category}
              </option>
            ))}
          </select>

          <label className="inline-flex h-control cursor-pointer items-center gap-1.5 rounded-md border border-border-strong bg-surface px-2.5 text-sm text-fg">
            <input
              type="checkbox"
              checked={Boolean(filter.onlyConflicts)}
              onChange={(e) => void setFilter({ onlyConflicts: e.target.checked })}
            />
            Only conflicts
          </label>

          <select
            aria-label="Conflict resolution policy"
            value={conflictPolicy}
            onChange={(e) => setConflictPolicy(e.target.value as ConflictPolicy)}
            className="h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          >
            {POLICY_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </div>

        {/* Post-execution summary */}
        {lastReport && (
          <div
            role="status"
            className="rounded-lg border border-border bg-success-subtle p-3 text-sm"
          >
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2 font-medium text-fg">
                <CheckCircle2 className="size-4 text-success" aria-hidden />
                <span>
                  Executed {plural(lastReport.completed.length, "change")}
                  {lastReport.queuedForReview > 0 &&
                    ` · ${plural(lastReport.queuedForReview, "item")} queued for review`}
                  {lastReport.skippedStale.length > 0 &&
                    ` · ${lastReport.skippedStale.length} skipped (stale/blocked)`}
                  {lastReport.failed.length > 0 && ` · ${lastReport.failed.length} failed`}
                </span>
              </div>
              <Link to="/activity" className="text-xs font-medium text-accent-fg hover:underline">
                Open Activity & Undo →
              </Link>
            </div>
            {lastReport.skippedStale.length > 0 && (
              <ul className="mt-2 list-disc space-y-0.5 pl-5 text-xs text-fg-muted">
                {lastReport.skippedStale.map((s) => (
                  <li key={s.itemId}>
                    <span className="font-mono">{s.sourcePath}</span>: {s.message}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

        {/* Items list */}
        {items.length === 0 ? (
          <EmptyState
            icon={Eye}
            title="No planned changes match"
            description="No files matched your active rules and filter criteria."
            action={
              (filter.search || filter.category || filter.onlyConflicts) ? (
                <Button onClick={() => void resetFilter()}>Clear filters</Button>
              ) : (
                <Link to="/rules">
                  <Button>Review rules</Button>
                </Link>
              )
            }
          />
        ) : (
          <div className="overflow-x-auto rounded-lg border border-border">
            <table className="w-full table-fixed border-collapse text-sm">
              <thead>
                <tr className="h-row border-b border-border bg-subtle text-left text-xs text-fg-muted">
                  <th className="w-10 px-3">
                    <input
                      type="checkbox"
                      aria-label="Select all ready items"
                      checked={allSelectableChecked}
                      disabled={selectableItems.length === 0}
                      onChange={() =>
                        allSelectableChecked ? deselectAll() : selectAllSelectable()
                      }
                    />
                  </th>
                  <th className="w-2/5 font-medium">File & Proposed Destination</th>
                  <th className="w-32 font-medium">Rule / Action</th>
                  <th className="w-24 font-medium">Size</th>
                  <th className="font-medium">Status & Checks</th>
                </tr>
              </thead>
              <tbody>
                {items.map((item: PlanItem) => {
                  const badge = STATUS_BADGE[item.status];
                  const blocked = item.status === "blocked";
                  const checked = !blocked && selectedSet.has(item.id);
                  return (
                    <tr
                      key={item.id}
                      className={cn(
                        "border-b border-border last:border-b-0",
                        blocked
                          ? "bg-danger-subtle/40"
                          : item.status === "conflict"
                            ? "bg-warning-subtle/30"
                            : "hover:bg-hover",
                      )}
                    >
                      <td className="px-3 align-top pt-2.5">
                        <input
                          type="checkbox"
                          aria-label={`Select ${item.originalName}`}
                          disabled={blocked}
                          checked={checked}
                          onChange={() => toggleItem(item.id)}
                        />
                      </td>
                      <td className="py-2.5 pr-4 align-top">
                        <div className="flex items-center gap-1.5 font-medium text-fg">
                          <span className="truncate">{item.originalName}</span>
                          {item.proposedName !== item.originalName && (
                            <span className="truncate text-xs text-accent-fg">
                              → {item.proposedName}
                            </span>
                          )}
                        </div>
                        <PathLabel path={item.sourcePath} />
                        <p className="mt-0.5 truncate font-mono text-xs text-fg-muted">
                          → {item.proposedDestination}
                        </p>
                      </td>
                      <td className="py-2.5 pr-2 align-top">
                        <p className="truncate font-medium text-fg">{item.ruleName}</p>
                        <p className="text-xs text-fg-muted">
                          {item.op.replaceAll("_", " ")}
                          {item.crossVolume ? " · cross-volume" : ""}
                        </p>
                      </td>
                      <td className="py-2.5 align-top text-fg-muted">
                        {formatBytes(item.size)}
                      </td>
                      <td className="py-2.5 pr-3 align-top">
                        <span
                          className={cn(
                            "inline-flex items-center gap-1 rounded-sm px-2 py-0.5 text-xs font-medium",
                            badge.className,
                          )}
                        >
                          {blocked ? (
                            <ShieldAlert className="size-3" aria-hidden />
                          ) : item.status === "conflict" ? (
                            <AlertTriangle className="size-3" aria-hidden />
                          ) : null}
                          {badge.label}
                        </span>
                        {item.conflicts.length > 0 && (
                          <ul className="mt-1 space-y-0.5 text-xs text-fg-muted">
                            {item.conflicts.map((c, idx) => (
                              <li key={idx}>{c.message}</li>
                            ))}
                          </ul>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            {total > items.length && (
              <div className="border-t border-border bg-subtle px-3 py-2 text-xs text-fg-muted">
                Showing {items.length} of {total.toLocaleString()} planned items.
              </div>
            )}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <PageHeader
        title="Preview"
        description="Simulate enabled rules across your monitored folders without modifying files until confirmed."
        actions={
          <>
            {summary && (
              <Button variant="ghost" onClick={() => void discard()} disabled={executing}>
                <Trash2 aria-hidden />
                Discard
              </Button>
            )}
            <Button
              variant="secondary"
              onClick={() => void generate()}
              disabled={status === "loading" || executing}
            >
              <RefreshCw aria-hidden />
              Refresh preview
            </Button>
            <Button
              variant="primary"
              disabled={!summary || selectedIds.length === 0 || executing}
              onClick={() => setConfirmOpen(true)}
            >
              <Play aria-hidden />
              {executing
                ? "Executing…"
                : `Execute selected (${selectedIds.length})`}
            </Button>
          </>
        }
      />

      <div className="min-h-0 flex-1 overflow-y-auto p-page">
        <div className="mx-auto max-w-content space-y-4">
          {actionError && <ErrorState error={actionError} onDismiss={clearActionError} />}
          {body}
        </div>
      </div>

      <ConfirmDialog
        open={confirmOpen}
        title="Apply planned changes?"
        confirmLabel="Apply changes"
        onConfirm={() => void handleConfirmExecute()}
        onCancel={() => setConfirmOpen(false)}
      >
        <p>
          FolderFlow will execute {plural(selectedIds.length, "selected operation")} with conflict
          policy <span className="font-medium text-fg">{conflictPolicy.replaceAll("_", " ")}</span>.
        </p>
        <p className="mt-2">
          Every file identity is re-verified immediately before execution, and completed moves can be
          undone from Activity.
        </p>
      </ConfirmDialog>
    </div>
  );
}

