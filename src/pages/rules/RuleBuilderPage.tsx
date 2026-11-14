import { AlertTriangle, ChevronLeft, FlaskConical, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router";
import { PageHeader } from "@/components/PageHeader";
import { ErrorState, LoadingState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { TextField } from "@/components/ui/text-field";
import { toUiError } from "@/services/result";
import {
  rulesService,
  type Action,
  type ConditionNode,
  type DateSource,
  type ExecutionMode,
  type RuleDraft,
  type RuleTestReport,
  type ValidationIssue,
} from "@/services/rules";
import { useFolders } from "@/stores/folders";
import { useRules } from "@/stores/rules";
import type { UiError } from "@/types/settings";
import { ActionRow, defaultActionForKind } from "./ActionRow";
import { ConditionGroup } from "./ConditionGroup";

const selectClass =
  "h-control w-full rounded-md border border-border-strong bg-surface px-2 text-sm text-fg focus-visible:border-border-focus";

function initialDraft(): RuleDraft {
  return {
    name: "New Rule",
    description: "",
    enabled: false,
    sourceFolders: [],
    conditions: {
      type: "all",
      children: [
        {
          type: "leaf",
          condition: {
            kind: "text",
            field: "extension",
            op: "equals",
            value: "pdf",
            case_sensitive: false,
          },
        },
      ],
    },
    actions: [
      {
        kind: "move_into_dynamic_folder",
        root: null,
        subpath_template: "Documents/Invoices/{year}/{month_name}",
      },
    ],
    dateSource: "modified",
    executionMode: "first_match",
  };
}

export function RuleBuilderPage() {
  const { id } = useParams();
  const isEdit = Boolean(id);
  const navigate = useNavigate();

  const folders = useFolders((s) => s.folders);
  const { rules, status: rulesStatus, load: loadRules, create, update } = useRules();

  const [draft, setDraft] = useState<RuleDraft>(() => initialDraft());
  const [loadedRuleId, setLoadedRuleId] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<UiError | null>(null);
  const [saveError, setSaveError] = useState<UiError | null>(null);
  const [saving, setSaving] = useState(false);
  const [issues, setIssues] = useState<ValidationIssue[]>([]);

  const [testFolderId, setTestFolderId] = useState<string>("");
  const [testing, setTesting] = useState(false);
  const [testReport, setTestReport] = useState<RuleTestReport | null>(null);
  const [testError, setTestError] = useState<UiError | null>(null);

  useEffect(() => {
    if (rulesStatus === "idle") void loadRules();
  }, [rulesStatus, loadRules]);

  useEffect(() => {
    if (!id) return;
    let active = true;
    rulesService
      .get(id)
      .then((rule) => {
        if (!active) return;
        setDraft({
          name: rule.name,
          description: rule.description ?? "",
          enabled: rule.enabled,
          sourceFolders: rule.sourceFolders,
          conditions: rule.conditions,
          actions: rule.actions,
          dateSource: rule.dateSource,
          executionMode: rule.executionMode,
        });
        setLoadError(null);
        setLoadedRuleId(id);
      })
      .catch((e: unknown) => {
        if (!active) return;
        setLoadError(toUiError(e));
        setLoadedRuleId(id);
      });
    return () => {
      active = false;
    };
  }, [id]);

  useEffect(() => {
    let active = true;
    rulesService
      .validate(
        {
          ...draft,
          description: draft.description?.trim() ? draft.description : null,
        },
        id ?? null,
      )
      .then((list) => {
        if (active) setIssues(list);
      })
      .catch(() => {
        if (active) setIssues([]);
      });
    return () => {
      active = false;
    };
  }, [draft, id]);

  const loadingRule = Boolean(id) && loadedRuleId !== id;
  if (loadingRule) return <LoadingState />;
  if (loadError) return <ErrorState error={loadError} onRetry={() => navigate("/rules")} />;

  const selectedTestFolderId = testFolderId || folders[0]?.id || "f1";

  const setField = <K extends keyof RuleDraft>(key: K, value: RuleDraft[K]) => {
    setDraft((prev) => ({ ...prev, [key]: value }));
    setSaveError(null);
  };

  const updateAction = (idx: number, nextAction: Action) => {
    const next = [...draft.actions];
    next[idx] = nextAction;
    setField("actions", next);
  };

  const removeAction = (idx: number) => {
    if (draft.actions.length <= 1) return;
    setField(
      "actions",
      draft.actions.filter((_, i) => i !== idx),
    );
  };

  const moveAction = (idx: number, direction: -1 | 1) => {
    const target = idx + direction;
    if (target < 0 || target >= draft.actions.length) return;
    const next = [...draft.actions];
    const a = next[idx];
    const b = next[target];
    if (!a || !b) return;
    next[idx] = b;
    next[target] = a;
    setField("actions", next);
  };

  const toggleSourceFolder = (folderId: string) => {
    const exists = draft.sourceFolders.includes(folderId);
    setField(
      "sourceFolders",
      exists
        ? draft.sourceFolders.filter((x) => x !== folderId)
        : [...draft.sourceFolders, folderId],
    );
  };

  const handleSave = async () => {
    setSaving(true);
    setSaveError(null);
    const payload = {
      ...draft,
      description: draft.description?.trim() ? draft.description.trim() : null,
    };
    const result = id ? await update(id, payload) : await create(payload);
    setSaving(false);
    if (result.ok) {
      navigate("/rules");
    } else {
      setSaveError(result.error);
    }
  };

  const handleTestFolder = async () => {
    setTesting(true);
    setTestError(null);
    try {
      const report = await rulesService.testFolder(
        {
          ...draft,
          description: draft.description?.trim() ? draft.description.trim() : null,
        },
        selectedTestFolderId,
      );
      setTestReport(report);
    } catch (e) {
      setTestError(toUiError(e));
    } finally {
      setTesting(false);
    }
  };

  const otherRules = rules.filter((r) => r.id !== id);
  const errorIssues = issues.filter((i) => i.severity === "error");
  const warningIssues = issues.filter((i) => i.severity === "warning");

  return (
    <>
      <PageHeader
        title={isEdit ? `Edit Rule: ${draft.name}` : "New Rule"}
        description="Build nested conditions and actions. Dry-run test against a monitored folder before enabling."
        actions={
          <>
            <Button variant="ghost" onClick={() => navigate("/rules")}>
              <ChevronLeft aria-hidden />
              Cancel
            </Button>
            <Button variant="primary" disabled={saving} onClick={() => void handleSave()}>
              {saving ? "Saving…" : "Save rule"}
            </Button>
          </>
        }
      />

      <div className="min-h-0 flex-1 overflow-y-auto p-page">
        <form
          className="max-w-content space-y-6"
          onSubmit={(e) => {
            e.preventDefault();
            void handleSave();
          }}
        >
          {saveError && <ErrorState error={saveError} onDismiss={() => setSaveError(null)} />}

          {(errorIssues.length > 0 || warningIssues.length > 0) && (
            <section aria-label="Validation issues" className="space-y-2">
              {errorIssues.map((issue, i) => (
                <div
                  key={`err-${i}`}
                  role="alert"
                  className="flex items-start gap-2 rounded-md border border-danger bg-danger-subtle px-3 py-2 text-xs text-danger"
                >
                  <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden />
                  <div>
                    <span className="font-medium">{issue.field}: </span>
                    <span>{issue.message}</span>
                  </div>
                </div>
              ))}
              {warningIssues.map((issue, i) => (
                <div
                  key={`warn-${i}`}
                  className="flex items-start gap-2 rounded-md border border-border bg-warning-subtle px-3 py-2 text-xs text-warning"
                >
                  <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden />
                  <div>
                    <span className="font-medium">{issue.field}: </span>
                    <span>{issue.message}</span>
                  </div>
                </div>
              ))}
            </section>
          )}

          <section aria-label="Rule details" className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <TextField
              id="rule-name"
              label="Rule name"
              value={draft.name}
              onChange={(e) => setField("name", e.target.value)}
              error={saveError?.field === "name" ? saveError.message : null}
            />
            <TextField
              id="rule-description"
              label="Description"
              placeholder="Optional summary of what this rule organizes"
              value={draft.description ?? ""}
              onChange={(e) => setField("description", e.target.value)}
            />
          </section>

          <section
            aria-label="Execution settings"
            className="grid grid-cols-1 gap-4 rounded-lg border border-border bg-subtle p-3 md:grid-cols-3"
          >
            <div className="flex items-center justify-between gap-3">
              <div>
                <label htmlFor="rule-builder-enabled" className="text-sm font-medium text-fg">
                  Rule enabled
                </label>
                <p className="text-xs text-fg-muted">
                  Disabled rules can be tested in preview without moving files.
                </p>
              </div>
              <Switch
                id="rule-builder-enabled"
                checked={draft.enabled}
                onChange={(v) => setField("enabled", v)}
              />
            </div>

            <div className="space-y-1">
              <label htmlFor="rule-exec-mode" className="block text-sm font-medium text-fg">
                Execution mode
              </label>
              <select
                id="rule-exec-mode"
                value={draft.executionMode}
                onChange={(e) => setField("executionMode", e.target.value as ExecutionMode)}
                className={selectClass}
              >
                <option value="first_match">Stop after this rule matches (First match)</option>
                <option value="continue">Continue evaluating lower-priority rules</option>
                <option value="stop_after_move">Stop only if this rule moves the file</option>
                <option value="stop_after_action">Stop after executing an action</option>
                <option value="sequential">Sequential pipeline</option>
              </select>
            </div>

            <div className="space-y-1">
              <label htmlFor="rule-date-source" className="block text-sm font-medium text-fg">
                Date variable source
              </label>
              <select
                id="rule-date-source"
                value={draft.dateSource}
                onChange={(e) => setField("dateSource", e.target.value as DateSource)}
                className={selectClass}
              >
                <option value="modified">File modified date (default)</option>
                <option value="created">File created date</option>
                <option value="added">Date added to FolderFlow</option>
                <option value="now">Current date when rule runs</option>
              </select>
            </div>
          </section>

          {folders.length > 0 && (
            <section aria-label="Source folders scope" className="space-y-2">
              <h2 className="text-sm font-semibold text-fg">Source folders</h2>
              <p className="text-xs text-fg-muted">
                Leave all unchecked to apply this rule to every monitored folder.
              </p>
              <div className="flex flex-wrap gap-3">
                {folders.map((f) => (
                  <label key={f.id} className="flex items-center gap-1.5 text-sm text-fg">
                    <input
                      type="checkbox"
                      checked={draft.sourceFolders.includes(f.id)}
                      onChange={() => toggleSourceFolder(f.id)}
                    />
                    {f.name}
                  </label>
                ))}
              </div>
            </section>
          )}

          <section aria-label="Conditions" className="space-y-2">
            <h2 className="text-sm font-semibold text-fg">Conditions (If)</h2>
            <ConditionGroup
              node={draft.conditions}
              onChange={(nextNode: ConditionNode) => setField("conditions", nextNode)}
            />
          </section>

          <section aria-label="Actions" className="space-y-2.5">
            <div className="flex items-center justify-between">
              <div>
                <h2 className="text-sm font-semibold text-fg">Actions (Then)</h2>
                <p className="text-xs text-fg-muted">
                  Actions run in order. At most one move action is allowed per rule.
                </p>
              </div>
              <Button
                size="sm"
                onClick={() =>
                  setField("actions", [...draft.actions, defaultActionForKind("rename")])
                }
              >
                <Plus aria-hidden />
                Add action
              </Button>
            </div>

            <div className="space-y-2">
              {draft.actions.map((act, idx) => (
                <ActionRow
                  key={idx}
                  index={idx}
                  action={act}
                  dateSource={draft.dateSource}
                  otherRules={otherRules}
                  canMoveUp={idx > 0}
                  canMoveDown={idx < draft.actions.length - 1}
                  canRemove={draft.actions.length > 1}
                  onChange={(nextAct) => updateAction(idx, nextAct)}
                  onMoveUp={() => moveAction(idx, -1)}
                  onMoveDown={() => moveAction(idx, 1)}
                  onRemove={() => removeAction(idx)}
                />
              ))}
            </div>
          </section>

          <section
            aria-label="Test rule"
            className="space-y-3 rounded-lg border border-border bg-subtle p-3"
          >
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div>
                <h2 className="text-sm font-semibold text-fg">Dry-run test on folder</h2>
                <p className="text-xs text-fg-muted">
                  Evaluates this draft against scanned files in a monitored folder without touching disk.
                </p>
              </div>
              <div className="flex items-center gap-2">
                {folders.length > 0 && (
                  <select
                    aria-label="Folder to test"
                    value={selectedTestFolderId}
                    onChange={(e) => setTestFolderId(e.target.value)}
                    className="h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
                  >
                    {folders.map((f) => (
                      <option key={f.id} value={f.id}>
                        {f.name}
                      </option>
                    ))}
                  </select>
                )}
                <Button disabled={testing} onClick={() => void handleTestFolder()}>
                  <FlaskConical aria-hidden />
                  {testing ? "Testing…" : "Test rule on folder"}
                </Button>
              </div>
            </div>

            {testError && <ErrorState error={testError} />}
            {testReport && (
              <div className="space-y-2 rounded-md border border-border bg-surface p-2.5 text-xs">
                <p className="font-medium text-fg">
                  {`Matched ${testReport.matchedFiles} of ${testReport.evaluatedFiles} files in ${testReport.folderName}`}
                </p>
                {testReport.sampleMatches.length > 0 && (
                  <ul className="divide-y divide-border">
                    {testReport.sampleMatches.map((m) => (
                      <li key={m.path} className="flex flex-wrap items-center justify-between gap-2 py-1">
                        <span className="font-mono text-fg-muted">{m.originalName}</span>
                        <span className="font-mono text-fg">
                          <span>{m.finalFilename}</span>
                          <span className="text-fg-subtle">{` → ${m.finalDirectory}`}</span>
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </section>
        </form>
      </div>
    </>
  );
}
