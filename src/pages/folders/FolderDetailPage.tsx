import { ChevronLeft, FolderX, Pause, Play, RefreshCw, ShieldAlert, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState, ErrorState, LoadingState } from "@/components/States";
import { StatusDot } from "@/components/StatusDot";
import { Button } from "@/components/ui/button";
import { categorySwatch } from "@/lib/categoryStyle";
import { cn } from "@/lib/cn";
import { folderStatus, isPaused } from "@/lib/folderStatus";
import { formatBytes, formatRelative, plural } from "@/lib/format";
import { foldersService, type Folder, type FolderSummary, type WatcherErrorView } from "@/services/folders";
import { toUiError } from "@/services/result";
import { useFolders } from "@/stores/folders";
import type { UiError } from "@/types/settings";
import { FolderSettingsForm } from "./FolderSettingsForm";
import { RemoveFolderDialog } from "./FoldersPage";

export function FolderDetailPage() {
  const { id = "" } = useParams();
  const navigate = useNavigate();
  const { folders, runtime, status, loadError, load, action, remove, actionError, clearActionError } = useFolders();
  const folder = folders.find((f) => f.id === id);
  const [removing, setRemoving] = useState<Folder | null>(null);

  if (status === "error" && loadError) return <ErrorState error={loadError} onRetry={() => void load()} />;
  if (status !== "ready") return <LoadingState />;
  if (!folder) {
    return (
      <EmptyState
        icon={FolderX}
        title="Folder not found"
        description="It may have been removed."
        action={
          <Link to="/folders" className="text-sm text-accent-fg">
            Back to folders
          </Link>
        }
      />
    );
  }

  const rt = runtime[folder.id];
  const st = folderStatus(folder, rt);
  const paused = isPaused(folder, rt);
  const problem = rt?.lastError ?? (st.tone === "error" ? folder.lastError : null);

  return (
    <>
      <PageHeader
        title={folder.name}
        description={folder.path}
        actions={
          <>
            <Button variant="ghost" onClick={() => navigate("/folders")}>
              <ChevronLeft aria-hidden />
              Folders
            </Button>
            {folder.enabled && (
              <Button onClick={() => void action(folder.id, paused ? "resume" : "pause")}>
                {paused ? <Play aria-hidden /> : <Pause aria-hidden />}
                {paused ? "Resume" : "Pause"}
              </Button>
            )}
            <Button onClick={() => void action(folder.id, "rescan")}>
              <RefreshCw aria-hidden />
              Rescan
            </Button>
            <Button variant="ghost" aria-label={`Remove ${folder.name}`} onClick={() => setRemoving(folder)}>
              <Trash2 aria-hidden />
            </Button>
          </>
        }
      />
      <div className="min-h-0 flex-1 overflow-y-auto p-page">
        <div className="max-w-content space-y-8">
          {actionError && <ErrorState error={actionError} onDismiss={clearActionError} />}

          <section aria-label="Status" className="space-y-2">
            <StatusDot tone={st.tone} label={st.label} className="text-base font-medium" />
            {problem && <p className="text-sm text-fg-muted">{problem}</p>}
            {st.needsPermission && (
              <Button onClick={() => void foldersService.openPrivacySettings()}>
                <ShieldAlert aria-hidden />
                Open System Settings
              </Button>
            )}
          </section>

          <FolderOverview folder={folder} />

          <section aria-label="Settings" className="max-w-lg">
            <h2 className="mb-4 text-sm font-semibold text-fg">Settings</h2>
            <FolderSettingsForm key={folder.id} folder={folder} />
          </section>

          <RecentErrors folderId={folder.id} />
        </div>
      </div>
      <RemoveFolderDialog folder={removing} onDone={() => setRemoving(null)} remove={remove} onRemoved={() => navigate("/folders")} />
    </>
  );
}

function FolderOverview({ folder }: { folder: Folder }) {
  const categories = useFolders((s) => s.categories);
  const [summary, setSummary] = useState<FolderSummary | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  useEffect(() => {
    let live = true;
    foldersService.summary(folder.id).then(
      (s) => live && setSummary(s),
      (e: unknown) => live && setError(toUiError(e)),
    );
    return () => {
      live = false;
    };
  }, [folder.id, folder.fileCount, folder.lastScannedAt]);

  const total = summary?.bytes ?? 0;
  return (
    <section aria-label="Overview" className="space-y-3">
      <p className="text-sm text-fg-muted">
        {plural(folder.fileCount, "file")} · {formatBytes(folder.totalBytes)} · Last scan: {formatRelative(folder.lastScannedAt)}
      </p>
      {error && <ErrorState error={error} />}
      {summary && summary.byCategory.length > 0 && (
        <ul className="max-w-lg space-y-2">
          {summary.byCategory.map((c) => {
            const meta = categories.find((x) => x.id === c.category);
            const pct = total > 0 ? Math.max(1, Math.round(((c.bytes ?? 0) / total) * 100)) : 0;
            return (
              <li key={c.category} className="space-y-1">
                <div className="flex items-center justify-between text-sm">
                  <span className="flex items-center gap-2 text-fg">
                    <span aria-hidden className={cn("size-2 rounded-full", categorySwatch(meta?.colorToken ?? `cat-${c.category}`))} />
                    {meta?.name ?? c.category}
                  </span>
                  <span className="flex gap-3 text-fg-muted">
                    <span>{plural(c.files, "file")}</span>
                    <span>{formatBytes(c.bytes)}</span>
                  </span>
                </div>
                <div className="h-1 rounded-full bg-muted">
                  <div className={cn("h-1 rounded-full", categorySwatch(meta?.colorToken ?? `cat-${c.category}`))} style={{ width: `${pct}%` }} />
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

function RecentErrors({ folderId }: { folderId: string }) {
  const [errors, setErrors] = useState<WatcherErrorView[]>([]);
  useEffect(() => {
    let live = true;
    foldersService.errors(folderId).then(
      (e) => live && setErrors(e),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [folderId]);
  if (errors.length === 0) return null;
  return (
    <section aria-label="Recent problems" className="max-w-lg">
      <h2 className="mb-2 text-sm font-semibold text-fg">Recent problems</h2>
      <ul className="divide-y divide-border rounded-md border border-border text-sm">
        {errors.slice(0, 10).map((e) => (
          <li key={`${e.occurredAt}-${e.code}`} className="px-3 py-2">
            <p className="text-fg">{e.message}</p>
            <p className="text-xs text-fg-muted">{formatRelative(e.occurredAt)}</p>
          </li>
        ))}
      </ul>
    </section>
  );
}
