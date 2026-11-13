import { FolderMinus, FolderPlus } from "lucide-react";
import { useState } from "react";
import { PathLabel } from "@/components/PathLabel";
import { ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { TextField } from "@/components/ui/text-field";
import { foldersService, type Folder } from "@/services/folders";
import { toUiError } from "@/services/result";
import { useFolders } from "@/stores/folders";
import type { UiError } from "@/types/settings";

interface FormState {
  name: string;
  enabled: boolean;
  recursive: boolean;
  exclude: string;
  include: string;
  excludedSubfolders: string[];
  stability: string;
}

const toForm = (f: Folder): FormState => ({
  name: f.name,
  enabled: f.enabled,
  recursive: f.recursive,
  exclude: f.settings.excludeExtensions.join(", "),
  include: f.settings.includeExtensions.join(", "),
  excludedSubfolders: f.settings.excludedSubfolders,
  stability: f.settings.stabilityIntervalSecs?.toString() ?? "",
});

/** Splits a comma/space separated list. Normalisation and validation happen in Rust. */
const splitList = (s: string) =>
  s
    .split(/[,\s]+/)
    .map((x) => x.trim())
    .filter(Boolean);

export function FolderSettingsForm({ folder }: { folder: Folder }) {
  const update = useFolders((s) => s.update);
  const [form, setForm] = useState<FormState>(() => toForm(folder));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const [saved, setSaved] = useState(false);

  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => {
    setForm((f) => ({ ...f, [k]: v }));
    setSaved(false);
  };
  const fieldError = (field: string) => (error?.field === field ? error.message : null);

  const addSubfolder = async () => {
    try {
      const picked = await foldersService.pickSubfolder(folder.id);
      if (picked && !form.excludedSubfolders.includes(picked)) set("excludedSubfolders", [...form.excludedSubfolders, picked]);
    } catch (e) {
      setError(toUiError(e));
    }
  };

  const save = async () => {
    setSaving(true);
    setError(null);
    const stability = form.stability.trim() === "" ? null : Number(form.stability);
    const result = await update(folder.id, {
      name: form.name,
      enabled: form.enabled,
      recursive: form.recursive,
      settings: {
        excludeExtensions: splitList(form.exclude),
        includeExtensions: splitList(form.include),
        excludedSubfolders: form.excludedSubfolders,
        stabilityIntervalSecs: stability !== null && Number.isFinite(stability) ? stability : null,
      },
    });
    setSaving(false);
    if (result.ok) {
      setForm(toForm(result.folder));
      setSaved(true);
    } else {
      setError(result.error);
    }
  };

  const generalError = error && !["name", "excludeExtensions", "includeExtensions", "excludedSubfolders", "stabilityIntervalSecs"].includes(error.field ?? "");

  return (
    <form
      className="space-y-5"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      {generalError && error && <ErrorState error={error} />}
      <TextField id="folder-name" label="Name" value={form.name} onChange={(e) => set("name", e.target.value)} error={fieldError("name")} />

      <div className="space-y-3">
        <ToggleRow id="folder-enabled" label="Monitoring enabled" description="Turn off to stop watching without removing the folder." checked={form.enabled} onChange={(v) => set("enabled", v)} />
        <ToggleRow id="folder-recursive" label="Include subfolders" description="Watch files in every folder inside this one." checked={form.recursive} onChange={(v) => set("recursive", v)} />
      </div>

      <TextField
        id="folder-exclude"
        label="Skip these extensions"
        placeholder="dmg, iso"
        hint="Comma-separated. Files with these extensions are ignored."
        value={form.exclude}
        onChange={(e) => set("exclude", e.target.value)}
        error={fieldError("excludeExtensions")}
      />
      <TextField
        id="folder-include"
        label="Only these extensions"
        placeholder="Leave empty for all files"
        value={form.include}
        onChange={(e) => set("include", e.target.value)}
        error={fieldError("includeExtensions")}
      />

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-fg">Skipped subfolders</span>
          <Button size="sm" onClick={() => void addSubfolder()}>
            <FolderPlus aria-hidden />
            Add subfolder…
          </Button>
        </div>
        {form.excludedSubfolders.length === 0 ? (
          <p className="text-xs text-fg-muted">None. Every subfolder is watched when “Include subfolders” is on.</p>
        ) : (
          <ul className="divide-y divide-border rounded-md border border-border">
            {form.excludedSubfolders.map((p) => (
              <li key={p} className="flex items-center gap-2 px-2 py-1.5">
                <PathLabel path={p} className="flex-1" />
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={`Stop skipping ${p}`}
                  onClick={() => set("excludedSubfolders", form.excludedSubfolders.filter((x) => x !== p))}
                >
                  <FolderMinus />
                </Button>
              </li>
            ))}
          </ul>
        )}
        {fieldError("excludedSubfolders") && <p className="text-xs text-danger">{fieldError("excludedSubfolders")}</p>}
      </div>

      <TextField
        id="folder-stability"
        label="Wait before processing (seconds)"
        inputMode="numeric"
        placeholder="Use app default"
        hint="How long a new file must stay unchanged before FolderFlow treats it as complete."
        value={form.stability}
        onChange={(e) => set("stability", e.target.value)}
        error={fieldError("stabilityIntervalSecs")}
      />

      <div className="flex items-center gap-3">
        <Button type="submit" variant="primary" disabled={saving}>
          {saving ? "Saving…" : "Save changes"}
        </Button>
        {saved && (
          <span role="status" className="text-sm text-success">
            Saved
          </span>
        )}
      </div>
    </form>
  );
}

function ToggleRow({ id, label, description, checked, onChange }: { id: string; label: string; description: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div>
        <label htmlFor={id} className="text-sm font-medium text-fg">
          {label}
        </label>
        <p className="text-xs text-fg-muted">{description}</p>
      </div>
      <Switch id={id} checked={checked} onChange={onChange} />
    </div>
  );
}
