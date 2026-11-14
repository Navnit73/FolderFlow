import { Switch } from "@/components/ui/switch";
import { useSettings } from "@/stores/settings";
import { SettingRow } from "./SettingRow";

export function MonitoringSection() {
  const settings = useSettings((s) => s.settings);
  const saveError = useSettings((s) => s.saveError);
  const update = useSettings((s) => s.update);
  if (!settings) return null;
  const m = settings.monitoring;

  const patch = (next: Partial<typeof m>) =>
    void update((cur) => ({ ...cur, monitoring: { ...cur.monitoring, ...next } }));

  return (
    <div>
      {saveError && (
        <p role="alert" className="mb-3 text-sm text-danger">
          {saveError.message}
        </p>
      )}
      <SettingRow
        label="File stability interval (seconds)"
        description="Wait duration between size/mtime checks before considering a new file ready (1–300s)."
      >
        <input
          aria-label="File stability interval (seconds)"
          type="number"
          min={1}
          max={300}
          value={m.stabilityIntervalSecs}
          onChange={(e) => patch({ stabilityIntervalSecs: Math.max(1, Number(e.target.value) || 1) })}
          className="h-control w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
        />
      </SettingRow>
      <SettingRow
        label="Stability checks"
        description="Consecutive unchanged checks required to mark a file stable (2–10)."
      >
        <input
          aria-label="Stability checks"
          type="number"
          min={2}
          max={10}
          value={m.stabilityChecks}
          onChange={(e) => patch({ stabilityChecks: Math.max(2, Number(e.target.value) || 2) })}
          className="h-control w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
        />
      </SettingRow>
      <SettingRow
        label="Reconciliation scan interval (minutes)"
        description="Periodic fallback folder sweep to catch missed OS filesystem events (5–1440 min)."
      >
        <input
          aria-label="Reconciliation scan interval (minutes)"
          type="number"
          min={5}
          max={1440}
          value={m.reconciliationIntervalMins}
          onChange={(e) => patch({ reconciliationIntervalMins: Math.max(5, Number(e.target.value) || 5) })}
          className="h-control w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
        />
      </SettingRow>
      <SettingRow
        label="Max concurrent file operations"
        description="Limit simultaneous background move/copy operations per folder (1–8)."
      >
        <input
          aria-label="Max concurrent file operations"
          type="number"
          min={1}
          max={8}
          value={m.maxConcurrentOps}
          onChange={(e) => patch({ maxConcurrentOps: Math.max(1, Number(e.target.value) || 1) })}
          className="h-control w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
        />
      </SettingRow>
      <SettingRow label="Recursive scanning by default" description="Default setting when adding new monitored folders.">
        <Switch
          id="mon-recursive-default"
          checked={m.recursiveByDefault}
          onChange={(checked) => patch({ recursiveByDefault: checked })}
        />
      </SettingRow>
      <SettingRow label="Ignore hidden files" description="Skip dotfiles such as .DS_Store cleanly.">
        <Switch
          id="mon-ignore-hidden"
          checked={m.ignoreHiddenFiles}
          onChange={(checked) => patch({ ignoreHiddenFiles: checked })}
        />
      </SettingRow>
      <SettingRow label="Ignore temporary download files" description="Skip .crdownload, .part, .download, and browser locks.">
        <Switch
          id="mon-ignore-temp"
          checked={m.ignoreTemporaryFiles}
          onChange={(checked) => patch({ ignoreTemporaryFiles: checked })}
        />
      </SettingRow>
    </div>
  );
}

