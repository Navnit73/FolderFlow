import { Plus, ShieldCheck, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Switch } from "@/components/ui/switch";
import { exclusionsService, type ExclusionEntry, type ExclusionKind } from "@/services/configIo";
import { toUiError } from "@/services/result";
import { useSettings } from "@/stores/settings";
import type { LogLevel, UiError } from "@/types/settings";
import { SettingRow } from "./SettingRow";

export function PrivacySection() {
  const settings = useSettings((s) => s.settings);
  const saveError = useSettings((s) => s.saveError);
  const update = useSettings((s) => s.update);

  const [exclusions, setExclusions] = useState<ExclusionEntry[]>([]);
  const [kind, setKind] = useState<ExclusionKind>("glob");
  const [pattern, setPattern] = useState("");
  const [exclError, setExclError] = useState<UiError | null>(null);

  useEffect(() => {
    let active = true;
    void exclusionsService
      .list(null)
      .then((all) => {
        if (active) setExclusions(all.filter((e) => e.folderId === null));
      })
      .catch((e) => {
        if (active) setExclError(toUiError(e));
      });
    return () => {
      active = false;
    };
  }, []);

  if (!settings) return null;
  const p = settings.privacy;
  const patch = (next: Partial<typeof p>) =>
    void update((cur) => ({ ...cur, privacy: { ...cur.privacy, ...next } }));

  const handleAddExclusion = async () => {
    setExclError(null);
    try {
      const created = await exclusionsService.create({
        folderId: null,
        path: pattern,
        exclusionType: kind,
      });
      setExclusions((prev) => [...prev, created]);
      setPattern("");
    } catch (e) {
      setExclError(toUiError(e));
    }
  };

  const handleDeleteExclusion = async (id: string) => {
    setExclError(null);
    try {
      await exclusionsService.remove(id);
      setExclusions((prev) => prev.filter((e) => e.id !== id));
    } catch (e) {
      setExclError(toUiError(e));
    }
  };

  return (
    <div className="space-y-6">
      <section className="flex items-start gap-3 rounded-xl border border-border bg-subtle p-4 text-sm">
        <ShieldCheck className="mt-0.5 size-5 shrink-0 text-success" aria-hidden />
        <div>
          <p className="font-medium text-fg">100% local & offline processing</p>
          <p className="mt-0.5 text-xs text-fg-muted">
            FolderFlow never uploads your files, paths, filenames, or hashes. All rules, duplicate scans,
            and SQLite ledgers stay strictly on your Mac.
          </p>
        </div>
      </section>

      <div>
        {saveError && <ErrorState error={saveError} />}
        <SettingRow
          label="Redact file paths in diagnostic logs"
          htmlFor="priv-redact"
          description="Replace user paths with anonymous hashes in rotated diagnostic log files."
        >
          <Switch
            id="priv-redact"
            checked={p.redactPathsInLogs}
            onChange={(redactPathsInLogs) => patch({ redactPathsInLogs })}
          />
        </SettingRow>
        <SettingRow label="Diagnostic log verbosity" description="Keep minimal unless troubleshooting.">
          <SegmentedControl<LogLevel>
            label="Diagnostic log verbosity"
            value={p.logLevel}
            onChange={(logLevel) => patch({ logLevel })}
            options={[
              { value: "normal", label: "Normal" },
              { value: "verbose", label: "Verbose" },
            ]}
          />
        </SettingRow>
        <SettingRow label="Diagnostic log retention (days)" description="Automatically prune older logs (1–90 days).">
          <input
            aria-label="Diagnostic log retention (days)"
            type="number"
            min={1}
            max={90}
            value={p.logRetentionDays}
            onChange={(e) => patch({ logRetentionDays: Math.max(1, Number(e.target.value) || 1) })}
            className="h-control w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          />
        </SettingRow>
      </div>

      <section className="space-y-3 rounded-xl border border-border bg-surface p-4">
        <div>
          <h3 className="text-sm font-semibold text-fg">Global exclusions</h3>
          <p className="text-xs text-fg-muted">
            Exclude paths, globs, or extensions across all monitored folders (e.g. `tmp`, `*.bak`).
          </p>
        </div>

        {exclError && <ErrorState error={exclError} onDismiss={() => setExclError(null)} />}

        <div className="flex flex-wrap gap-2">
          <select
            aria-label="Exclusion type"
            value={kind}
            onChange={(e) => setKind(e.target.value as ExclusionKind)}
            className="h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          >
            <option value="glob">Glob</option>
            <option value="extension">Extension</option>
            <option value="path">Absolute path</option>
          </select>
          <input
            type="text"
            aria-label="Exclusion pattern or path"
            value={pattern}
            onChange={(e) => setPattern(e.target.value)}
            placeholder={
              kind === "extension"
                ? "iso"
                : kind === "glob"
                  ? "*.tmp"
                  : "/Users/you/Downloads/Keep"
            }
            className="h-control flex-1 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          />
          <Button variant="secondary" disabled={!pattern.trim()} onClick={() => void handleAddExclusion()}>
            <Plus aria-hidden />
            Add exclusion
          </Button>
        </div>

        {exclusions.length === 0 ? (
          <p className="text-xs text-fg-subtle">No global exclusions configured.</p>
        ) : (
          <ul className="divide-y divide-border rounded-lg border border-border">
            {exclusions.map((ex) => (
              <li key={ex.id} className="flex items-center justify-between px-3 py-2 text-sm">
                <div className="min-w-0">
                  <span className="mr-2 rounded-sm bg-muted px-1.5 py-0.5 text-2xs uppercase text-fg-muted">
                    {ex.exclusionType}
                  </span>
                  <span className="font-mono text-xs text-fg">{ex.path}</span>
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label={`Remove exclusion ${ex.path}`}
                  onClick={() => void handleDeleteExclusion(ex.id)}
                >
                  <Trash2 aria-hidden />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
