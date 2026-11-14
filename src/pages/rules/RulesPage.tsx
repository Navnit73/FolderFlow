import {
  ArrowDown,
  ArrowUp,
  Copy,
  ListChecks,
  Plus,
  Sparkles,
  Trash2,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState, ErrorState, LoadingState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { DropdownMenu } from "@/components/ui/dropdown-menu";
import { Switch } from "@/components/ui/switch";
import { formatRelative } from "@/lib/format";
import type { ExecutionMode, Rule, RuleTemplate } from "@/services/rules";
import { useRules } from "@/stores/rules";

const EXECUTION_MODE_LABELS: Record<ExecutionMode, string> = {
  first_match: "First match",
  continue: "Continue matching",
  stop_after_move: "Stop after move",
  stop_after_action: "Stop after action",
  sequential: "Sequential",
};

export function RulesPage() {
  const navigate = useNavigate();
  const {
    rules,
    presets,
    status,
    loadError,
    actionError,
    load,
    createFromPreset,
    setEnabled,
    moveRule,
    duplicate,
    remove,
    clearActionError,
  } = useRules();
  const [deleting, setDeleting] = useState<Rule | null>(null);

  useEffect(() => {
    if (status === "idle") void load();
  }, [status, load]);

  const headerActions = (
    <>
      <StarterRulesMenu presets={presets} onSelect={(id) => void createFromPreset(id)} variant="secondary" />
      <Button variant="primary" onClick={() => navigate("/rules/new")}>
        <Plus aria-hidden />
        New rule
      </Button>
    </>
  );

  let body;
  if (status === "error" && loadError) {
    body = <ErrorState error={loadError} onRetry={() => void load()} />;
  } else if (status !== "ready") {
    body = <LoadingState />;
  } else if (rules.length === 0) {
    body = (
      <EmptyState
        icon={ListChecks}
        title="No rules yet"
        description="Create a custom rule or start from a smart template. Starter rules begin disabled so you can review them before anything moves."
        action={
          <div className="flex items-center gap-2">
            <StarterRulesMenu
              presets={presets}
              onSelect={(id) => void createFromPreset(id)}
              variant="secondary"
            />
            <Button variant="primary" onClick={() => navigate("/rules/new")}>
              <Plus aria-hidden />
              New rule
            </Button>
          </div>
        }
      />
    );
  } else {
    body = (
      <table className="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr className="h-row border-b border-border text-left text-xs text-fg-muted">
            <th className="w-16 font-medium">Active</th>
            <th className="w-2/5 font-medium">Rule</th>
            <th className="font-medium">Mode</th>
            <th className="font-medium">Matched</th>
            <th className="font-medium">Last run</th>
            <th className="sr-only">Actions</th>
          </tr>
        </thead>
        <tbody>
          {rules.map((r, idx) => {
            const switchId = `rule-enabled-${r.id}`;
            return (
              <tr key={r.id} className="border-b border-border last:border-b-0 hover:bg-hover">
                <td className="py-2 pr-2">
                  <label htmlFor={switchId} className="sr-only">
                    {`Enable ${r.name}`}
                  </label>
                  <Switch
                    id={switchId}
                    checked={r.enabled}
                    onChange={(next) => void setEnabled(r.id, next)}
                  />
                </td>
                <td className="py-2 pr-4">
                  <div className="flex items-center gap-2">
                    <Link
                      to={`/rules/${r.id}`}
                      className="truncate font-medium text-fg hover:text-accent-fg"
                    >
                      {r.name}
                    </Link>
                    {r.origin === "preset" && (
                      <span className="rounded-sm bg-accent-subtle px-1.5 py-0.5 text-2xs font-medium text-accent-fg">
                        Preset
                      </span>
                    )}
                  </div>
                  {r.description && (
                    <p className="truncate text-xs text-fg-muted">{r.description}</p>
                  )}
                </td>
                <td className="text-fg-muted">{EXECUTION_MODE_LABELS[r.executionMode]}</td>
                <td className="text-fg-muted">{`${r.statMatched} matched`}</td>
                <td className="text-fg-muted">{formatRelative(r.lastRunAt)}</td>
                <td>
                  <div className="flex justify-end gap-0.5">
                    <Button
                      size="icon"
                      variant="ghost"
                      disabled={idx === 0}
                      aria-label={`Move ${r.name} up`}
                      title="Move up"
                      onClick={() => void moveRule(r.id, "up")}
                    >
                      <ArrowUp />
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      disabled={idx === rules.length - 1}
                      aria-label={`Move ${r.name} down`}
                      title="Move down"
                      onClick={() => void moveRule(r.id, "down")}
                    >
                      <ArrowDown />
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      aria-label={`Duplicate ${r.name}`}
                      title="Duplicate"
                      onClick={() => void duplicate(r.id)}
                    >
                      <Copy />
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      aria-label={`Delete ${r.name}`}
                      title="Delete"
                      onClick={() => setDeleting(r)}
                    >
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
      <PageHeader
        title="Rules"
        description="Rules evaluate in priority order from top to bottom. Disabled rules never move files."
        actions={headerActions}
      />
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-page">
        {actionError && <ErrorState error={actionError} onDismiss={clearActionError} />}
        {body}
      </div>
      <ConfirmDialog
        open={deleting !== null}
        title={`Delete “${deleting?.name ?? ""}”?`}
        confirmLabel="Delete rule"
        danger
        onCancel={() => setDeleting(null)}
        onConfirm={() => {
          const id = deleting?.id;
          setDeleting(null);
          if (id) void remove(id);
        }}
      >
        This removes the rule from FolderFlow. Existing files that were already organized stay where they are.
      </ConfirmDialog>
    </>
  );
}

function StarterRulesMenu({
  presets,
  onSelect,
  variant = "secondary",
}: {
  presets: RuleTemplate[];
  onSelect: (presetId: string) => void;
  variant?: "primary" | "secondary";
}) {
  if (presets.length === 0) return null;
  return (
    <DropdownMenu
      variant={variant}
      label={
        <>
          <Sparkles aria-hidden />
          Add starter rule
        </>
      }
      items={presets.map((p) => ({
        label: p.name,
        onSelect: () => onSelect(p.id),
      }))}
    />
  );
}
