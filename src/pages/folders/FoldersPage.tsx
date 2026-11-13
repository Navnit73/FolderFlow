import { FolderOpen, Pause, Play, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { PageHeader } from "@/components/PageHeader";
import { PathLabel } from "@/components/PathLabel";
import { EmptyState, ErrorState, LoadingState } from "@/components/States";
import { StatusDot } from "@/components/StatusDot";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { folderStatus, isPaused } from "@/lib/folderStatus";
import { formatBytes, formatRelative, plural } from "@/lib/format";
import type { Folder } from "@/services/folders";
import { useFolders } from "@/stores/folders";
import { AddFolderMenu } from "./AddFolderMenu";

export function FoldersPage() {
  const { folders, runtime, paused, status, loadError, actionError, load, action, setPaused, remove, clearActionError } = useFolders();
  const [removing, setRemoving] = useState<Folder | null>(null);

  const header = (
    <PageHeader
      title="Folders"
      description="Folders FolderFlow watches. Nothing is moved until you create and enable rules."
      actions={
        <>
          {folders.length > 0 && (
            <Button onClick={() => void setPaused(!paused)}>
              {paused ? <Play aria-hidden /> : <Pause aria-hidden />}
              {paused ? "Resume all" : "Pause all"}
            </Button>
          )}
          <AddFolderMenu />
        </>
      }
    />
  );

  let body;
  if (status === "error" && loadError) {
    body = <ErrorState error={loadError} onRetry={() => void load()} />;
  } else if (status !== "ready") {
    body = <LoadingState />;
  } else if (folders.length === 0) {
    body = (
      <EmptyState
        icon={FolderOpen}
        title="No folders yet"
        description="Add a folder like Downloads to start. FolderFlow only watches — it won't move anything until you enable rules."
        action={<AddFolderMenu variant="secondary" />}
      />
    );
  } else {
    body = (
      <table className="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr className="h-row border-b border-border text-left text-xs text-fg-muted">
            <th className="w-2/5 font-medium">Folder</th>
            <th className="font-medium">Status</th>
            <th className="font-medium">Files</th>
            <th className="font-medium">Size</th>
            <th className="font-medium">Last scan</th>
            <th className="sr-only">Actions</th>
          </tr>
        </thead>
        <tbody>
          {folders.map((f) => {
            const rt = runtime[f.id];
            const st = folderStatus(f, rt);
            const fPaused = isPaused(f, rt);
            return (
              <tr key={f.id} className="border-b border-border last:border-b-0 hover:bg-hover">
                <td className="py-2 pr-4">
                  <Link to={`/folders/${f.id}`} className="block truncate font-medium text-fg hover:text-accent-fg">
                    {f.name}
                  </Link>
                  <PathLabel path={f.path} />
                </td>
                <td>
                  <StatusDot tone={st.tone} label={st.label} />
                </td>
                <td className="text-fg-muted">{plural(f.fileCount, "file")}</td>
                <td className="text-fg-muted">{formatBytes(f.totalBytes)}</td>
                <td className="text-fg-muted">{formatRelative(f.lastScannedAt)}</td>
                <td>
                  <div className="flex justify-end gap-0.5">
                    {f.enabled && (
                      <Button
                        size="icon"
                        variant="ghost"
                        aria-label={`${fPaused ? "Resume" : "Pause"} ${f.name}`}
                        title={fPaused ? "Resume" : "Pause"}
                        onClick={() => void action(f.id, fPaused ? "resume" : "pause")}
                      >
                        {fPaused ? <Play /> : <Pause />}
                      </Button>
                    )}
                    <Button size="icon" variant="ghost" aria-label={`Rescan ${f.name}`} title="Rescan" onClick={() => void action(f.id, "rescan")}>
                      <RefreshCw />
                    </Button>
                    <Button size="icon" variant="ghost" aria-label={`Remove ${f.name}`} title="Stop monitoring" onClick={() => setRemoving(f)}>
                      <Trash2 />
                    </Button>
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    );
  }

  return (
    <>
      {header}
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-page">
        {actionError && <ErrorState error={actionError} onDismiss={clearActionError} />}
        {body}
      </div>
      <RemoveFolderDialog folder={removing} onDone={() => setRemoving(null)} remove={remove} />
    </>
  );
}

export function RemoveFolderDialog({
  folder,
  onDone,
  remove,
  onRemoved,
}: {
  folder: Folder | null;
  onDone: () => void;
  remove: (id: string) => Promise<boolean>;
  onRemoved?: () => void;
}) {
  return (
    <ConfirmDialog
      open={folder !== null}
      title={`Stop monitoring “${folder?.name ?? ""}”?`}
      confirmLabel="Stop monitoring"
      danger
      onCancel={onDone}
      onConfirm={() => {
        const id = folder?.id;
        onDone();
        if (id) void remove(id).then((ok) => ok && onRemoved?.());
      }}
    >
      FolderFlow will forget this folder. Your files stay where they are — nothing is moved or deleted.
    </ConfirmDialog>
  );
}
