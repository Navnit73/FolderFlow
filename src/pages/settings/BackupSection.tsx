import { Download, FileUp, Upload } from "lucide-react";
import { useState, type ChangeEvent } from "react";
import { ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { triggerTextDownload } from "@/lib/download";
import { plural } from "@/lib/format";
import {
  configIoService,
  type ConfigImportMode,
  type ConfigImportPreview,
  type ConfigImportReport,
} from "@/services/configIo";
import { toUiError } from "@/services/result";
import { useCategories } from "@/stores/categories";
import { useDashboard } from "@/stores/dashboard";
import { useDuplicates } from "@/stores/duplicates";
import { useFolders } from "@/stores/folders";
import { useRules } from "@/stores/rules";
import { useSettings } from "@/stores/settings";
import type { UiError } from "@/types/settings";

const MAX_BACKUP_IMPORT_BYTES = 5 * 1024 * 1024;

export function BackupSection() {
  const [rawJson, setRawJson] = useState("");
  const [preview, setPreview] = useState<ConfigImportPreview | null>(null);
  const [mode, setMode] = useState<ConfigImportMode>("merge");
  const [report, setReport] = useState<ConfigImportReport | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [busy, setBusy] = useState(false);

  const loadSettings = useSettings((s) => s.load);
  const loadFolders = useFolders((s) => s.load);
  const loadRules = useRules((s) => s.load);
  const loadCategories = useCategories((s) => s.load);

  const handleExport = async () => {
    setError(null);
    setBusy(true);
    try {
      const json = await configIoService.exportConfig();
      triggerTextDownload("folderflow-backup.json", json, "application/json;charset=utf-8;");
      setRawJson(json);
    } catch (e) {
      setError(toUiError(e));
    } finally {
      setBusy(false);
    }
  };

  const runPreview = async (jsonText: string, targetMode: ConfigImportMode = mode) => {
    setRawJson(jsonText);
    setPreview(null);
    setReport(null);
    setError(null);
    if (!jsonText.trim()) return;
    if (new TextEncoder().encode(jsonText).length > MAX_BACKUP_IMPORT_BYTES) {
      setError({
        code: "validation",
        message: "Backup file exceeds the 5 MB safety limit.",
        hint: "Choose a smaller exported configuration JSON file.",
        field: null,
      });
      return;
    }
    try {
      const p = await configIoService.previewImport(jsonText, targetMode);
      setPreview(p);
    } catch (e) {
      setError(toUiError(e));
    }
  };

  const handleModeChange = (nextMode: ConfigImportMode) => {
    setMode(nextMode);
    if (rawJson.trim()) {
      void runPreview(rawJson, nextMode);
    }
  };

  const handlePickNative = async () => {
    setError(null);
    try {
      const text = await configIoService.pickImportFile();
      if (text) await runPreview(text, mode);
    } catch (e) {
      setError(toUiError(e));
    }
  };

  const handleFileInput = async (e: ChangeEvent<HTMLInputElement>) => {
    const input = e.target;
    const file = input.files?.[0];
    if (!file) return;
    setError(null);
    try {
      if (file.size > MAX_BACKUP_IMPORT_BYTES) {
        setError({
          code: "validation",
          message: "Backup file exceeds the 5 MB safety limit.",
          hint: "Choose a smaller exported configuration JSON file.",
          field: null,
        });
        return;
      }
      const text = await file.text();
      await runPreview(text, mode);
    } catch (err) {
      setError(toUiError(err));
    } finally {
      input.value = "";
    }
  };

  const handleApply = async () => {
    if (!rawJson.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const rep = await configIoService.applyImport(rawJson, mode);
      setReport(rep);
      setPreview(null);
      await Promise.all([
        loadSettings(),
        loadFolders(),
        loadRules(),
        loadCategories(),
        useDashboard.getState().refresh(),
        useDuplicates.getState().status === "ready" ? useDuplicates.getState().load() : Promise.resolve(),
      ]);
    } catch (e) {
      setError(toUiError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-6">
      {error && <ErrorState error={error} onDismiss={() => setError(null)} />}

      <section className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-border bg-subtle p-4">
        <div>
          <h3 className="text-sm font-semibold text-fg">Export configuration backup</h3>
          <p className="mt-0.5 text-xs text-fg-muted">
            Versioned JSON bundle with settings, monitored folders, categories, exclusions, and rules.
          </p>
        </div>
        <Button variant="primary" disabled={busy} onClick={() => void handleExport()}>
          <Download aria-hidden />
          Export JSON backup
        </Button>
      </section>

      <section className="space-y-4 rounded-xl border border-border bg-surface p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h3 className="text-sm font-semibold text-fg">Import & preview configuration</h3>
            <p className="mt-0.5 text-xs text-fg-muted">
              Every bundle is schema-validated, paths are checked against protected system roots, and imported
              rules start disabled.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="secondary" onClick={() => void handlePickNative()}>
              <FileUp aria-hidden />
              Choose backup file
            </Button>
            <label className="inline-flex h-control-sm cursor-pointer items-center gap-1 rounded-md border border-border-strong bg-surface px-2.5 text-xs font-medium text-fg hover:bg-hover">
              <Upload className="size-3.5" aria-hidden />
              Load .json
              <input
                type="file"
                accept=".json,application/json"
                onChange={(e) => void handleFileInput(e)}
                className="sr-only"
              />
            </label>
          </div>
        </div>

        <div className="space-y-1">
          <label htmlFor="config-import-json" className="block text-xs font-medium text-fg-muted">
            Backup JSON
          </label>
          <textarea
            id="config-import-json"
            rows={5}
            value={rawJson}
            onChange={(e) => setRawJson(e.target.value)}
            placeholder='Paste exported FolderFlow JSON or load a backup file…'
            className="w-full rounded-md border border-border-strong bg-surface p-2 font-mono text-xs text-fg"
          />
          <div className="flex justify-end">
            <Button
              size="sm"
              variant="secondary"
              disabled={!rawJson.trim()}
              onClick={() => void runPreview(rawJson)}
            >
              Preview diff
            </Button>
          </div>
        </div>

        {preview && (
          <div className="space-y-3 rounded-lg border border-border bg-subtle p-4 text-sm">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <p className="font-semibold text-fg">
                Import preview (schema v{preview.schemaVersion})
              </p>
              <SegmentedControl<ConfigImportMode>
                label="Import mode"
                value={mode}
                onChange={handleModeChange}
                options={[
                  { value: "merge", label: "Merge" },
                  { value: "replace", label: "Replace" },
                ]}
              />
            </div>

            {mode === "replace" && (
              <p className="rounded-md border border-border bg-danger-subtle p-2.5 text-xs text-danger">
                Replace mode removes existing folders, custom categories, exclusions, and rules before importing this backup.
              </p>
            )}

            <ul className="grid gap-1 text-xs text-fg-muted sm:grid-cols-2">
              <li>Folders: +{preview.foldersToAdd.length} add, {preview.foldersToUpdate.length} update</li>
              <li>Categories: +{preview.categoriesToAdd.length} add, {preview.categoriesToUpdate.length} update</li>
              <li>Exclusions: +{preview.exclusionsToAdd.length} add</li>
              <li>Rules: +{preview.rulesToAdd.length} add, {preview.rulesToUpdate.length} update</li>
              <li>Settings sections changed: {preview.settingsChanges.length}</li>
              <li>
                Safety: {plural(preview.forcedDisabledRuleCount, "imported rule")} will be created disabled
              </li>
            </ul>

            {preview.warnings.length > 0 && (
              <div className="rounded-md border border-border bg-warning-subtle p-2.5 text-xs text-warning">
                {preview.warnings.map((w, i) => (
                  <p key={i}>{w}</p>
                ))}
              </div>
            )}

            <div className="flex justify-end">
              <Button variant="primary" disabled={busy} onClick={() => void handleApply()}>
                {busy ? "Applying…" : `Apply (${mode})`}
              </Button>
            </div>
          </div>
        )}

        {report && (
          <div role="status" className="rounded-md border border-border bg-success-subtle p-3 text-sm text-fg">
            Imported configuration ({report.mode}): {plural(report.foldersImported, "folder")},{" "}
            {plural(report.categoriesImported, "category")}, {plural(report.exclusionsImported, "exclusion")},{" "}
            {plural(report.rulesImportedDisabled, "rule")} (all imported rules start disabled).
          </div>
        )}
      </section>
    </div>
  );
}

