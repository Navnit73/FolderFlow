import { FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";
import { ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { toUiError } from "@/services/result";
import { rulesService } from "@/services/rules";
import { useSettings } from "@/stores/settings";
import type { ConflictPolicy, RuleExecutionMode, UiError } from "@/types/settings";
import { SettingRow } from "./SettingRow";

export function OrganizationSection() {
  const settings = useSettings((s) => s.settings);
  const saveError = useSettings((s) => s.saveError);
  const update = useSettings((s) => s.update);
  const [templateOverride, setTemplateOverride] = useState<string | null>(null);
  const [previewSample, setPreviewSample] = useState<string | null>(null);
  const [previewErr, setPreviewErr] = useState<string | null>(null);
  const [pickError, setPickError] = useState<UiError | null>(null);
  const templateDraft = templateOverride ?? settings?.organization.namingTemplate ?? "{original_name}";

  useEffect(() => {
    let active = true;
    void rulesService
      .templatePreview({
        template: templateDraft,
        isFilename: true,
        dateSource: "modified",
        sample: {
          name: "invoice-q4.pdf",
          category: "Documents",
          sourceFolder: "Downloads",
          createdMs: 1_710_000_000_000,
          modifiedMs: 1_710_000_000_000,
        },
      })
      .then((res) => {
        if (!active) return;
        setPreviewSample(res.valid ? res.rendered : null);
        setPreviewErr(res.valid ? null : res.error);
      })
      .catch(() => {
        if (active) setPreviewErr(null);
      });
    return () => {
      active = false;
    };
  }, [templateDraft]);

  if (!settings) return null;
  const o = settings.organization;
  const patch = (next: Partial<typeof o>) =>
    void update((cur) => ({ ...cur, organization: { ...cur.organization, ...next } }));

  const pickDefaultDest = async () => {
    setPickError(null);
    try {
      const picked = await rulesService.pickDestinationFolder();
      if (picked) patch({ defaultDestination: picked });
    } catch (e) {
      setPickError(toUiError(e));
    }
  };

  return (
    <div>
      {saveError && (
        <p role="alert" className="mb-3 text-sm text-danger">
          {saveError.message}
        </p>
      )}
      {pickError && (
        <div className="mb-3">
          <ErrorState error={pickError} onDismiss={() => setPickError(null)} />
        </div>
      )}
      <SettingRow
        label="Default destination root"
        description={o.defaultDestination ?? "Uses relative paths inside your home or chosen rule destination."}
      >
        <div className="flex items-center gap-2">
          {o.defaultDestination && (
            <Button size="sm" variant="ghost" onClick={() => patch({ defaultDestination: null })}>
              Clear
            </Button>
          )}
          <Button size="sm" variant="secondary" onClick={() => void pickDefaultDest()}>
            <FolderOpen aria-hidden />
            Choose folder
          </Button>
        </div>
      </SettingRow>

      <SettingRow
        label="Conflict resolution policy"
        description="How FolderFlow behaves when a target file with the same name already exists."
      >
        <select
          aria-label="Conflict resolution policy"
          value={o.conflictPolicy}
          onChange={(e) => patch({ conflictPolicy: e.target.value as ConflictPolicy })}
          className="h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
        >
          <option value="auto_rename">Auto-rename cleanly</option>
          <option value="skip">Skip conflicting file</option>
          <option value="ask">Queue for Review</option>
          <option value="keep_both">Keep both</option>
        </select>
      </SettingRow>

      <SettingRow
        label="Default execution mode"
        description="Default rule chaining strategy when creating new rules."
      >
        <select
          aria-label="Default execution mode"
          value={o.executionMode}
          onChange={(e) => patch({ executionMode: e.target.value as RuleExecutionMode })}
          className="h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
        >
          <option value="first_match">First match</option>
          <option value="continue">Continue matching</option>
          <option value="stop_after_move">Stop after move</option>
          <option value="stop_after_action">Stop after action</option>
          <option value="sequential">Sequential</option>
        </select>
      </SettingRow>

      <div className="space-y-2 border-b border-border py-4">
        <label htmlFor="org-naming-template" className="block text-sm font-medium text-fg">
          Default filename template
        </label>
        <div className="flex gap-2">
          <input
            id="org-naming-template"
            type="text"
            value={templateDraft}
            onChange={(e) => setTemplateOverride(e.target.value)}
            className="h-control flex-1 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          />
          <Button
            variant="secondary"
            disabled={templateDraft === o.namingTemplate}
            onClick={() => {
              patch({ namingTemplate: templateDraft });
              setTemplateOverride(null);
            }}
          >
            Apply
          </Button>
        </div>
        {previewErr ? (
          <p className="text-xs text-danger">{previewErr}</p>
        ) : (
          previewSample && <p className="text-xs text-fg-muted">Preview: {previewSample}</p>
        )}
      </div>

      <SettingRow
        label="Auto-create missing destination folders"
        description="Automatically create dynamic subfolders such as 2026/Invoices when moving files."
      >
        <Switch
          id="org-autocreate"
          checked={o.autoCreateFolders}
          onChange={(checked) => patch({ autoCreateFolders: checked })}
        />
      </SettingRow>
    </div>
  );
}
