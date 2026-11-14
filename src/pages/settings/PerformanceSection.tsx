import { Database } from "lucide-react";
import { useState } from "react";
import { ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Switch } from "@/components/ui/switch";
import { formatBytes } from "@/lib/format";
import { dashboardService, type DbMaintenanceReport } from "@/services/dashboard";
import { toUiError } from "@/services/result";
import { useSettings } from "@/stores/settings";
import type { CpuPreference, UiError } from "@/types/settings";
import { SettingRow } from "./SettingRow";

export function PerformanceSection() {
  const settings = useSettings((s) => s.settings);
  const saveError = useSettings((s) => s.saveError);
  const update = useSettings((s) => s.update);
  const [maintReport, setMaintReport] = useState<DbMaintenanceReport | null>(null);
  const [maintError, setMaintError] = useState<UiError | null>(null);
  const [runningMaint, setRunningMaint] = useState(false);

  if (!settings) return null;
  const p = settings.performance;
  const patch = (next: Partial<typeof p>) =>
    void update((cur) => ({ ...cur, performance: { ...cur.performance, ...next } }));

  const handleMaintenance = async () => {
    setRunningMaint(true);
    setMaintError(null);
    try {
      const rep = await dashboardService.runMaintenance();
      setMaintReport(rep);
    } catch (e) {
      setMaintError(toUiError(e));
    } finally {
      setRunningMaint(false);
    }
  };

  return (
    <div>
      {saveError && (
        <p role="alert" className="mb-3 text-sm text-danger">
          {saveError.message}
        </p>
      )}
      <SettingRow label="CPU preference" description="Balance background responsiveness against throughput.">
        <SegmentedControl<CpuPreference>
          label="CPU preference"
          value={p.cpuPreference}
          onChange={(cpuPreference) => patch({ cpuPreference })}
          options={[
            { value: "low", label: "Low" },
            { value: "balanced", label: "Balanced" },
            { value: "high", label: "High" },
          ]}
        />
      </SettingRow>
      <SettingRow
        label="Background processing"
        htmlFor="perf-bg"
        description="Continue scheduled and watcher organization tasks cleanly in the background."
      >
        <Switch
          id="perf-bg"
          checked={p.backgroundProcessing}
          onChange={(backgroundProcessing) => patch({ backgroundProcessing })}
        />
      </SettingRow>
      <SettingRow
        label="Compute BLAKE3 content hashes during folder scans"
        htmlFor="perf-hash-scan"
        description="Pre-warms the duplicate cache during directory scans for instant duplicate detection."
      >
        <Switch
          id="perf-hash-scan"
          checked={p.hashOnScan}
          onChange={(hashOnScan) => patch({ hashOnScan })}
        />
      </SettingRow>
      <SettingRow label="Max scan concurrency" description="Worker threads used when scanning directories (1–16).">
        <input
          aria-label="Max scan concurrency"
          type="number"
          min={1}
          max={16}
          value={p.maxScanConcurrency}
          onChange={(e) => patch({ maxScanConcurrency: Math.max(1, Number(e.target.value) || 1) })}
          className="h-control w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
        />
      </SettingRow>

      <SettingRow
        label="Database maintenance"
        description="Run SQLite WAL checkpoint, ANALYZE, and VACUUM to compact storage."
      >
        <Button variant="secondary" disabled={runningMaint} onClick={() => void handleMaintenance()}>
          <Database aria-hidden />
          {runningMaint ? "Optimizing…" : "Run VACUUM & ANALYZE"}
        </Button>
      </SettingRow>

      {maintError && (
        <div className="mt-2">
          <ErrorState error={maintError} onDismiss={() => setMaintError(null)} />
        </div>
      )}

      {maintReport && (
        <p role="status" className="mt-2 rounded-md border border-border bg-success-subtle px-3 py-2 text-xs text-fg">
          Database compacted: {formatBytes(maintReport.sizeBytesBefore)} →{" "}
          {formatBytes(maintReport.sizeBytesAfter)} (freelist {maintReport.freelistPagesBefore} →{" "}
          {maintReport.freelistPagesAfter} pages).
        </p>
      )}
    </div>
  );
}

