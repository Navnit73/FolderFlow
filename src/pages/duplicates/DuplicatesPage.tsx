import {
  CheckCircle2,
  Copy,
  Download,
  ExternalLink,
  EyeOff,
  FolderInput,
  Search,
  Sparkles,
  Trash2,
} from "lucide-react";
import { useEffect, useState } from "react";
import { PageHeader } from "@/components/PageHeader";
import { PathLabel } from "@/components/PathLabel";
import { EmptyState, ErrorState, LoadingState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/cn";
import { triggerTextDownload } from "@/lib/download";
import { formatBytes, formatRelative, plural } from "@/lib/format";
import type { DuplicateGroupSummary } from "@/services/duplicates";
import { useDuplicates } from "@/stores/duplicates";

type ConfirmTrashState =
  | { kind: "extras"; group: DuplicateGroupSummary; count: number }
  | { kind: "single"; group: DuplicateGroupSummary; fileRecordId: string; filename: string }
  | null;

export function DuplicatesPage() {
  const {
    groups,
    totals,
    selectedGroupId,
    includeIgnored,
    differentNamesOnly,
    search,
    scanning,
    scanProgress,
    lastScanReport,
    status,
    loadError,
    actionError,
    load,
    scan,
    setIncludeIgnored,
    setDifferentNamesOnly,
    setSearch,
    selectGroup,
    applyAction,
    exportReport,
    revealFile,
    clearActionError,
  } = useDuplicates();

  const [trashConfirm, setTrashConfirm] = useState<ConfirmTrashState>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (status === "idle") void load();
  }, [status, load]);

  const visibleGroups = differentNamesOnly
    ? groups.filter((g) => g.sameContentDifferentNames)
    : groups;
  const selectedGroup =
    visibleGroups.find((g) => g.id === selectedGroupId) ?? visibleGroups[0] ?? null;

  const handleScan = async () => {
    setNotice(null);
    const rep = await scan();
    if (rep) {
      setNotice(
        `Scanned ${plural(rep.filesScanned, "file")}: found ${plural(rep.groupsFound, "duplicate group")} (${formatBytes(rep.totalWastedBytes)} reclaimable).`,
      );
    }
  };

  const handleExport = async (format: "csv" | "json") => {
    const content = await exportReport(format);
    if (content !== null) {
      triggerTextDownload(
        `folderflow-duplicates.${format}`,
        content,
        format === "csv" ? "text/csv;charset=utf-8;" : "application/json;charset=utf-8;",
      );
      setNotice(`Exported duplicates report (${format.toUpperCase()}).`);
    }
  };

  const handleMoveExtrasToReview = async (group: DuplicateGroupSummary, memberIds?: string[]) => {
    const rep = await applyAction({
      kind: "move_to_review",
      group_id: group.id,
      file_record_ids: memberIds ?? null,
      review_dir: null,
    });
    if (rep) {
      setNotice(`Moved ${plural(rep.affected, "duplicate copy")} to Duplicate Review.`);
    }
  };

  const handleConfirmTrash = async () => {
    if (!trashConfirm) return;
    const target = trashConfirm;
    setTrashConfirm(null);
    const ids = target.kind === "single" ? [target.fileRecordId] : null;
    const rep = await applyAction({
      kind: "trash",
      group_id: target.group.id,
      file_record_ids: ids,
      confirmed: true,
    });
    if (rep) {
      setNotice(`Moved ${plural(rep.affected, "duplicate file")} to Trash.`);
    }
  };

  const headerActions = (
    <>
      <Button variant="secondary" onClick={() => void handleExport("csv")} disabled={groups.length === 0}>
        <Download aria-hidden />
        Export CSV
      </Button>
      <Button variant="secondary" onClick={() => void handleExport("json")} disabled={groups.length === 0}>
        <Download aria-hidden />
        Export JSON
      </Button>
      <Button variant="primary" onClick={() => void handleScan()} disabled={scanning}>
        <Sparkles aria-hidden />
        {scanning ? "Scanning…" : "Scan for duplicates"}
      </Button>
    </>
  );

  let body;
  if (status === "error" && loadError) {
    body = <ErrorState error={loadError} onRetry={() => void load()} />;
  } else if (status !== "ready") {
    body = <LoadingState label="Loading duplicate groups…" />;
  } else if (visibleGroups.length === 0 && !scanning) {
    body = (
      <EmptyState
        icon={Copy}
        title={groups.length === 0 ? "No duplicates found yet" : "No groups match current filter"}
        description={
          groups.length === 0
            ? "Scan your monitored folders to find exact content duplicates safely without modifying any files."
            : "Adjust search text or toggles to see other detected duplicate groups."
        }
        action={
          <Button variant="primary" onClick={() => void handleScan()}>
            <Sparkles aria-hidden />
            Scan monitored folders
          </Button>
        }
      />
    );
  } else {
    const nonKeeperCount = selectedGroup
      ? selectedGroup.members.filter((m) => !m.isKeeper).length
      : 0;

    body = (
      <div className="space-y-4">
        {/* Totals summary bar */}
        <section className="grid gap-3 sm:grid-cols-4">
          <div className="rounded-xl border border-border bg-subtle p-3">
            <p className="text-xs text-fg-muted">Active groups</p>
            <p className="mt-0.5 text-lg font-semibold tabular-nums text-fg">{totals.activeGroups}</p>
          </div>
          <div className="rounded-xl border border-border bg-subtle p-3">
            <p className="text-xs text-fg-muted">Duplicate copies</p>
            <p className="mt-0.5 text-lg font-semibold tabular-nums text-fg">{totals.totalDuplicateCopies}</p>
          </div>
          <div className="rounded-xl border border-border bg-subtle p-3">
            <p className="text-xs text-fg-muted">Reclaimable space</p>
            <p className="mt-0.5 text-lg font-semibold tabular-nums text-fg">
              {formatBytes(totals.totalWastedBytes)}
            </p>
          </div>
          <div className="rounded-xl border border-border bg-subtle p-3">
            <p className="text-xs text-fg-muted">Ignored groups</p>
            <p className="mt-0.5 text-lg font-semibold tabular-nums text-fg">{totals.ignoredGroups}</p>
          </div>
        </section>

        {/* Master-detail split */}
        <div className="grid gap-4 lg:grid-cols-12">
          <section aria-label="Duplicate groups" className="lg:col-span-5">
            <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-surface">
              {visibleGroups.map((group) => {
                const isSelected = selectedGroup?.id === group.id;
                const leadName = group.members[0]?.filename ?? group.contentHash.slice(0, 10);
                return (
                  <li key={group.id}>
                    <button
                      type="button"
                      onClick={() => selectGroup(group.id)}
                      className={cn(
                        "flex w-full items-start justify-between gap-3 px-4 py-3 text-left transition-colors",
                        isSelected ? "bg-selected" : "hover:bg-hover",
                      )}
                    >
                      <div className="min-w-0">
                        <p className="truncate text-sm font-medium text-fg">{leadName}</p>
                        <p className="mt-0.5 text-xs text-fg-muted">
                          {plural(group.copyCount, "copy", "copies")} · {formatBytes(group.fileSize)} each
                        </p>
                        <div className="mt-1 flex flex-wrap gap-1.5">
                          {group.sameContentDifferentNames && (
                            <span className="rounded-full bg-info-subtle px-2 py-0.5 text-2xs font-medium text-info">
                              Same content, different names
                            </span>
                          )}
                          {group.ignored && (
                            <span className="rounded-full bg-muted px-2 py-0.5 text-2xs font-medium text-fg-muted">
                              Ignored
                            </span>
                          )}
                        </div>
                      </div>
                      <span className="shrink-0 text-xs font-medium tabular-nums text-fg-muted">
                        {formatBytes(group.wastedBytes)} wasted
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </section>

          {selectedGroup && (
            <section
              aria-label="Selected duplicate group details"
              className="space-y-4 rounded-xl border border-border bg-surface p-4 lg:col-span-7"
            >
              <div className="flex flex-wrap items-start justify-between gap-3 border-b border-border pb-3">
                <div>
                  <h2 className="text-base font-semibold text-fg">
                    {selectedGroup.members[0]?.filename ?? "Duplicate group"}
                  </h2>
                  <p className="mt-0.5 text-xs text-fg-muted">
                    BLAKE3 {selectedGroup.contentHash.slice(0, 16)}… · {formatBytes(selectedGroup.fileSize)} ·{" "}
                    {plural(selectedGroup.copyCount, "copy", "copies")}
                  </p>
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() =>
                      void applyAction({
                        kind: "ignore",
                        group_id: selectedGroup.id,
                        ignored: !selectedGroup.ignored,
                      })
                    }
                  >
                    <EyeOff aria-hidden />
                    {selectedGroup.ignored ? "Unignore group" : "Ignore group"}
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={nonKeeperCount === 0}
                    onClick={() => void handleMoveExtrasToReview(selectedGroup)}
                  >
                    <FolderInput aria-hidden />
                    Move extras to Review
                  </Button>
                  <Button
                    size="sm"
                    variant="danger"
                    disabled={nonKeeperCount === 0}
                    onClick={() =>
                      setTrashConfirm({
                        kind: "extras",
                        group: selectedGroup,
                        count: nonKeeperCount,
                      })
                    }
                  >
                    <Trash2 aria-hidden />
                    Trash non-keepers
                  </Button>
                </div>
              </div>

              <ul className="divide-y divide-border">
                {selectedGroup.members.map((member) => (
                  <li key={member.fileRecordId} className="flex flex-wrap items-center justify-between gap-3 py-3">
                    <div className="min-w-0 flex-1 space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="truncate text-sm font-medium text-fg">{member.filename}</span>
                        {member.isKeeper && (
                          <span className="inline-flex items-center gap-1 rounded-full bg-success-subtle px-2 py-0.5 text-xs font-medium text-success">
                            <CheckCircle2 className="size-3" aria-hidden />
                            Keeper
                          </span>
                        )}
                        {!member.exists && (
                          <span className="rounded-full bg-warning-subtle px-2 py-0.5 text-xs font-medium text-warning">
                            Missing on disk
                          </span>
                        )}
                      </div>
                      <PathLabel path={member.path} className="text-xs text-fg-muted" />
                      <p className="text-xs text-fg-subtle">
                        {formatBytes(member.size)} · Modified {formatRelative(member.modifiedMs)}
                      </p>
                    </div>
                    <div className="flex shrink-0 flex-wrap items-center gap-1.5">
                      {!member.isKeeper && (
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() =>
                            void applyAction({
                              kind: "keep",
                              group_id: selectedGroup.id,
                              file_record_id: member.fileRecordId,
                            })
                          }
                        >
                          Keep this
                        </Button>
                      )}
                      <Button
                        size="sm"
                        variant="ghost"
                        aria-label={`Reveal ${member.filename} in Finder`}
                        onClick={() => void revealFile(member.fileRecordId)}
                      >
                        <ExternalLink aria-hidden />
                        Finder
                      </Button>
                      {!member.isKeeper && (
                        <>
                          <Button
                            size="sm"
                            variant="secondary"
                            onClick={() =>
                              void handleMoveExtrasToReview(selectedGroup, [member.fileRecordId])
                            }
                          >
                            Review
                          </Button>
                          <Button
                            size="sm"
                            variant="danger"
                            aria-label={`Move ${member.filename} to Trash`}
                            disabled={selectedGroup.members.length <= 1}
                            onClick={() =>
                              setTrashConfirm({
                                kind: "single",
                                group: selectedGroup,
                                fileRecordId: member.fileRecordId,
                                filename: member.filename,
                              })
                            }
                          >
                            <Trash2 aria-hidden />
                            Trash
                          </Button>
                        </>
                      )}
                    </div>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </div>
      </div>
    );
  }

  return (
    <>
      <PageHeader
        title="Duplicates"
        description="Byte-accurate BLAKE3 duplicate detection. Keep one copy safely; move extras to Review or macOS Trash."
        actions={headerActions}
      />
      <div className="min-h-0 flex-1 overflow-y-auto p-page">
        <div className="mx-auto max-w-content space-y-4">
          {/* Search and filter controls */}
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border bg-subtle px-4 py-2.5">
            <div className="relative flex-1">
              <Search className="pointer-events-none absolute left-2.5 top-2.5 size-4 text-fg-subtle" aria-hidden />
              <input
                type="search"
                data-ff-search="true"
                aria-label="Filter duplicates"
                value={search}
                onChange={(e) => void setSearch(e.target.value)}
                placeholder="Search duplicate filename or path…"
                className="h-control w-full rounded-md border border-border-strong bg-surface pl-8 pr-3 text-sm text-fg placeholder:text-fg-subtle"
              />
            </div>
            <label className="flex items-center gap-2 text-xs font-medium text-fg">
              <Switch
                id="dup-diff-names"
                checked={differentNamesOnly}
                onChange={setDifferentNamesOnly}
              />
              Different names only
            </label>
            <label className="flex items-center gap-2 text-xs font-medium text-fg">
              <Switch
                id="dup-show-ignored"
                checked={includeIgnored}
                onChange={(v) => void setIncludeIgnored(v)}
              />
              Show ignored
            </label>
          </div>

          {scanning && (
            <div role="status" className="rounded-xl border border-border bg-subtle p-4 text-sm">
              <div className="flex items-center justify-between text-xs font-medium text-fg">
                <span>Scanning duplicate candidates…</span>
                <span className="tabular-nums text-fg-muted">
                  {scanProgress
                    ? `${scanProgress.hashed} / ${scanProgress.total} hashed (${formatBytes(scanProgress.bytes)})`
                    : "Prefiltering by size…"}
                </span>
              </div>
              {scanProgress && scanProgress.total > 0 && (
                <div className="mt-2 h-2 w-full overflow-hidden rounded-full bg-muted">
                  <div
                    className="h-2 rounded-full bg-accent"
                    style={{
                      width: `${Math.min(100, Math.round((scanProgress.hashed / scanProgress.total) * 100))}%`,
                    }}
                  />
                </div>
              )}
            </div>
          )}

          {notice && (
            <div role="status" className="rounded-md border border-border bg-success-subtle px-3 py-2 text-sm text-fg">
              {notice}
            </div>
          )}
          {lastScanReport && lastScanReport.cacheHits > 0 && !scanning && (
            <p className="text-xs text-fg-subtle">
              Last scan reused {plural(lastScanReport.cacheHits, "cached BLAKE3 hash")} and skipped{" "}
              {plural(lastScanReport.uniqueSizeSkipped, "unique-size file")}.
            </p>
          )}
          {actionError && <ErrorState error={actionError} onDismiss={clearActionError} />}
          {body}
        </div>
      </div>

      <ConfirmDialog
        open={trashConfirm !== null}
        title="Move duplicates to Trash?"
        confirmLabel="Move to Trash"
        danger
        onConfirm={() => void handleConfirmTrash()}
        onCancel={() => setTrashConfirm(null)}
      >
        {trashConfirm?.kind === "extras" ? (
          <p>
            Move {plural(trashConfirm.count, "non-keeper copy")} to the macOS Trash? The designated Keeper
            copy will remain untouched.
          </p>
        ) : trashConfirm ? (
          <p>
            Move <span className="font-medium text-fg">{trashConfirm.filename}</span> to the macOS Trash?
            At least one verified copy stays preserved in this group.
          </p>
        ) : null}
      </ConfirmDialog>
    </>
  );
}

