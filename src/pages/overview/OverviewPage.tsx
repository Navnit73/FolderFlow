import {
  Activity,
  Copy,
  Eye,
  FolderPlus,
  History,
  ListChecks,
  Pause,
  Play,
  Plus,
  RefreshCw,
} from "lucide-react";
import { useEffect } from "react";
import { Link, useNavigate } from "react-router";
import { events } from "@/bindings";
import { PageHeader } from "@/components/PageHeader";
import { ErrorState, LoadingState } from "@/components/States";
import { StatusDot } from "@/components/StatusDot";
import { Button } from "@/components/ui/button";
import { categorySwatch } from "@/lib/categoryStyle";
import { cn } from "@/lib/cn";
import { formatBytes, formatRelative, plural } from "@/lib/format";
import { useDashboard } from "@/stores/dashboard";
import { useFolders } from "@/stores/folders";

function statusPillClass(status: string): string {
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

export function OverviewPage() {
  const navigate = useNavigate();
  const { stats, status, error, load, refresh } = useDashboard();
  const {
    folders,
    paused,
    status: foldersStatus,
    load: loadFolders,
    setPaused,
  } = useFolders();

  useEffect(() => {
    if (status === "idle") void load();
    if (foldersStatus === "idle") void loadFolders();
  }, [status, load, foldersStatus, loadFolders]);

  useEffect(() => {
    let active = true;
    const unlistenPromises = [
      events.folderActivityEvent.listen(() => {
        if (active) void refresh();
      }),
      events.folderStatusEvent.listen(() => {
        if (active) void refresh();
      }),
    ];
    return () => {
      active = false;
      for (const p of unlistenPromises) {
        void p.then((unlisten) => unlisten());
      }
    };
  }, [refresh]);

  const headerActions = (
    <>
      <Button variant="secondary" onClick={() => void refresh()}>
        <RefreshCw aria-hidden />
        Refresh
      </Button>
      <Button
        variant={paused ? "primary" : "secondary"}
        onClick={() => void setPaused(!paused)}
        disabled={folders.length === 0}
      >
        {paused ? <Play aria-hidden /> : <Pause aria-hidden />}
        {paused ? "Resume monitoring" : "Pause all"}
      </Button>
    </>
  );

  let body;
  if (status === "error" && error) {
    body = <ErrorState error={error} onRetry={() => void load()} />;
  } else if (status !== "ready" || !stats) {
    body = <LoadingState label="Loading dashboard metrics…" />;
  } else {
    const todayCount = stats.filesOrganizedToday ?? 0;
    const weekCount = stats.filesOrganizedThisWeek ?? 0;
    const totalCount = stats.filesOrganizedTotal ?? 0;
    const totalBytesOrg = stats.totalBytesOrganized ?? 0;
    const trackedFiles = stats.totalTrackedFiles ?? 0;
    const trackedBytes = stats.totalTrackedBytes ?? 0;
    const dupWasted = stats.duplicateWastedBytes ?? 0;
    const maxDaily = Math.max(1, ...stats.dailyActivity.map((d) => d.count ?? 0));

    const monitoringTone =
      stats.activeFoldersCount === 0 ? "idle" : paused ? "paused" : "active";
    const monitoringLabel =
      stats.activeFoldersCount === 0
        ? "No active folders"
        : paused
          ? "All monitoring paused"
          : `Watching ${plural(stats.activeFoldersCount, "folder")}`;

    body = (
      <div className="space-y-6">
        {/* Top status strip & quick actions */}
        <section className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-border bg-subtle p-4">
          <div className="space-y-1">
            <StatusDot tone={monitoringTone} label={monitoringLabel} className="text-sm font-medium" />
            <p className="text-xs text-fg-muted">
              {stats.activeRulesCount} of {stats.totalRulesCount} rules active ·{" "}
              {stats.activeFoldersCount} of {stats.totalFoldersCount} folders enabled
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="secondary" onClick={() => navigate("/folders")}>
              <FolderPlus aria-hidden />
              Add folder
            </Button>
            <Button size="sm" variant="secondary" onClick={() => navigate("/rules/new")}>
              <Plus aria-hidden />
              Create rule
            </Button>
            <Button size="sm" variant="secondary" onClick={() => navigate("/preview")}>
              <Eye aria-hidden />
              Preview
            </Button>
            <Button size="sm" variant="secondary" onClick={() => navigate("/duplicates")}>
              <Copy aria-hidden />
              Find duplicates
            </Button>
          </div>
        </section>

        {/* Primary KPI grid (all values queried from SQLite) */}
        <section aria-label="Key metrics" className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          <div className="rounded-xl border border-border bg-surface p-4">
            <p className="text-xs font-medium text-fg-muted">Organized today</p>
            <p className="mt-1 text-2xl font-semibold tabular-nums text-fg">
              {todayCount.toLocaleString()}
            </p>
            <p className="mt-1 text-xs text-fg-subtle">
              {weekCount.toLocaleString()} this week · {totalCount.toLocaleString()} all time
            </p>
          </div>

          <div className="rounded-xl border border-border bg-surface p-4">
            <p className="text-xs font-medium text-fg-muted">Space organized</p>
            <p className="mt-1 text-2xl font-semibold tabular-nums text-fg">
              {formatBytes(totalBytesOrg)}
            </p>
            <p className="mt-1 text-xs text-fg-subtle">
              {plural(trackedFiles, "tracked file")} ({formatBytes(trackedBytes)})
            </p>
          </div>

          <div className="rounded-xl border border-border bg-surface p-4">
            <p className="text-xs font-medium text-fg-muted">Duplicates reclaimable</p>
            <p className="mt-1 text-2xl font-semibold tabular-nums text-fg">
              {formatBytes(dupWasted)}
            </p>
            <p className="mt-1 text-xs text-fg-subtle">
              {plural(stats.duplicateGroupsCount, "active duplicate group")}
            </p>
          </div>

          <div className="rounded-xl border border-border bg-surface p-4">
            <p className="text-xs font-medium text-fg-muted">Needs attention</p>
            <p className="mt-1 text-2xl font-semibold tabular-nums text-fg">
              {(stats.pendingReviewCount + stats.failedOperationsCount).toLocaleString()}
            </p>
            <p className="mt-1 text-xs text-fg-subtle">
              {stats.pendingReviewCount} pending review · {stats.failedOperationsCount} failed
            </p>
          </div>
        </section>

        {/* 7-day trend & Storage by category */}
        <div className="grid gap-4 lg:grid-cols-2">
          <section className="rounded-xl border border-border bg-surface p-4">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold text-fg">Last 7 days</h2>
              <span className="text-xs text-fg-muted">Files organized per day</span>
            </div>
            <div className="mt-4 grid grid-cols-7 items-end gap-2">
              {stats.dailyActivity.map((day) => {
                const count = day.count ?? 0;
                const pct = count === 0 ? 4 : Math.max(10, Math.round((count / maxDaily) * 100));
                return (
                  <div key={day.date} className="flex flex-col items-center gap-1.5">
                    <span className="text-xs tabular-nums text-fg-muted">{count}</span>
                    <div className="flex h-24 w-full items-end rounded-md bg-subtle p-1">
                      <div
                        className="w-full rounded-sm bg-accent"
                        style={{ height: `${pct}%` }}
                        title={`${day.date}: ${count}`}
                      />
                    </div>
                    <span className="text-2xs text-fg-subtle">{day.date.slice(5)}</span>
                  </div>
                );
              })}
            </div>
          </section>

          <section className="rounded-xl border border-border bg-surface p-4">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold text-fg">Storage by category</h2>
              <Link to="/categories" className="text-xs text-accent-fg hover:underline">
                Manage categories
              </Link>
            </div>
            {stats.categoryBreakdown.length === 0 ? (
              <p className="mt-6 text-sm text-fg-muted">
                No tracked files yet. Add a monitored folder to inspect storage by category.
              </p>
            ) : (
              <div className="mt-4 space-y-2.5">
                {stats.categoryBreakdown.slice(0, 6).map((cat) => {
                  const catBytes = cat.totalBytes ?? 0;
                  const catFiles = cat.fileCount ?? 0;
                  const pct =
                    trackedBytes > 0
                      ? Math.max(3, Math.round((catBytes / trackedBytes) * 100))
                      : 0;
                  return (
                    <div key={cat.categoryId} className="space-y-1">
                      <div className="flex items-center justify-between text-xs">
                        <span className="flex items-center gap-2 font-medium text-fg">
                          <span className={cn("size-2 rounded-full", categorySwatch(cat.colorToken))} />
                          {cat.categoryName}
                        </span>
                        <span className="tabular-nums text-fg-muted">
                          {plural(catFiles, "file")} · {formatBytes(catBytes)}
                        </span>
                      </div>
                      <div className="h-1.5 w-full overflow-hidden rounded-full bg-subtle">
                        <div
                          className={cn("h-1.5 rounded-full", categorySwatch(cat.colorToken))}
                          style={{ width: `${pct}%` }}
                        />
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </section>
        </div>

        {/* Rule performance & Recent activity */}
        <div className="grid gap-4 lg:grid-cols-2">
          <section className="rounded-xl border border-border bg-surface p-4">
            <div className="flex items-center justify-between">
              <h2 className="flex items-center gap-1.5 text-sm font-semibold text-fg">
                <ListChecks className="size-4 text-fg-muted" aria-hidden />
                Rule performance
              </h2>
              <Link to="/rules" className="text-xs text-accent-fg hover:underline">
                View all rules
              </Link>
            </div>
            {stats.topRules.length === 0 ? (
              <p className="mt-4 text-sm text-fg-muted">
                No rules created yet. Create a custom rule or starter preset in Rules.
              </p>
            ) : (
              <ul className="mt-3 divide-y divide-border text-sm">
                {stats.topRules.map((rule) => (
                  <li key={rule.ruleId} className="flex items-center justify-between py-2">
                    <div className="min-w-0">
                      <Link
                        to={`/rules/${rule.ruleId}`}
                        className="truncate font-medium text-fg hover:text-accent-fg"
                      >
                        {rule.ruleName}
                      </Link>
                      <p className="text-xs text-fg-subtle">
                        {rule.enabled ? "Enabled" : "Disabled"} · Last run{" "}
                        {formatRelative(rule.lastRunAtMs)}
                      </p>
                    </div>
                    <div className="shrink-0 text-right text-xs">
                      <p className="font-medium tabular-nums text-fg">
                        {plural(rule.matchedOps ?? 0, "file")}
                      </p>
                      <p className="tabular-nums text-fg-muted">
                        {formatBytes(rule.bytesOrganized)}
                        {rule.failedOps > 0 ? ` · ${rule.failedOps} failed` : ""}
                      </p>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="rounded-xl border border-border bg-surface p-4">
            <div className="flex items-center justify-between">
              <h2 className="flex items-center gap-1.5 text-sm font-semibold text-fg">
                <History className="size-4 text-fg-muted" aria-hidden />
                Recent activity
              </h2>
              <Link to="/activity" className="text-xs text-accent-fg hover:underline">
                Open activity log
              </Link>
            </div>
            {stats.recentOperations.length === 0 ? (
              <p className="mt-4 text-sm text-fg-muted">
                No file operations recorded yet.
              </p>
            ) : (
              <ul className="mt-3 divide-y divide-border text-sm">
                {stats.recentOperations.map((op) => (
                  <li key={op.id} className="flex items-center justify-between gap-2 py-2">
                    <div className="min-w-0">
                      <p className="truncate font-medium text-fg">{op.newName || op.originalName}</p>
                      <p className="truncate text-xs text-fg-subtle">
                        {op.operationType.replaceAll("_", " ")} · {formatRelative(op.completedAt ?? op.startedAt)}
                      </p>
                    </div>
                    <span
                      className={cn(
                        "shrink-0 rounded-full px-2 py-0.5 text-xs font-medium",
                        statusPillClass(op.status),
                      )}
                    >
                      {op.status}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      </div>
    );
  }

  return (
    <>
      <PageHeader
        title="Overview"
        description="Real-time status, organized volume, category distribution, and recent file activity."
        actions={headerActions}
      />
      <div className="min-h-0 flex-1 overflow-y-auto p-page">
        <div className="mx-auto max-w-content">
          <div className="sr-only">
            <Activity aria-hidden />
          </div>
          {body}
        </div>
      </div>
    </>
  );
}

