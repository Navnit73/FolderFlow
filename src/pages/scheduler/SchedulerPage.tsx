import { CalendarClock, Clock, Pencil, Play, Plus, Trash2 } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { commands, type MonitoredFolder_Serialize, type Rule_Serialize } from "@/bindings";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState, ErrorState, LoadingState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/cn";
import { formatRelative, plural } from "@/lib/format";
import type {
  CatchUpPolicy,
  Schedule,
  ScheduleInput,
  ScheduleRunOutcome,
  ScheduleType,
  Weekday,
} from "@/services/schedules";
import { useSchedules } from "@/stores/schedules";

const WEEKDAY_OPTIONS: { value: Weekday; label: string }[] = [
  { value: "mon", label: "Mon" },
  { value: "tue", label: "Tue" },
  { value: "wed", label: "Wed" },
  { value: "thu", label: "Thu" },
  { value: "fri", label: "Fri" },
  { value: "sat", label: "Sat" },
  { value: "sun", label: "Sun" },
];

const INTERVAL_PRESETS: { secs: number; label: string }[] = [
  { secs: 300, label: "Every 5 min" },
  { secs: 900, label: "Every 15 min" },
  { secs: 1800, label: "Every 30 min" },
  { secs: 3600, label: "Every 1 hour" },
];

const TYPE_LABELS: Record<ScheduleType, string> = {
  realtime: "Real-time",
  interval: "Interval",
  daily: "Daily",
  weekly: "Weekly",
  on_launch: "On launch",
  manual: "Manual only",
};

const OUTCOME_LABELS: Record<ScheduleRunOutcome["status"], string> = {
  executed: "Executed scheduled run",
  no_matching_files: "No matching files",
  skipped_overlap: "Skipped: run already in progress",
  skipped_quiet_hours: "Skipped: inside quiet hours",
  skipped_missed: "Skipped missed run per policy",
  disabled: "Target rule or folder is disabled",
};

const HHMM_RE = /^([01]\d|2[0-3]):[0-5]\d$/;

interface FormDraft {
  id: string | null;
  folderId: string;
  ruleId: string;
  scheduleType: ScheduleType;
  intervalMins: number;
  time: string;
  weekdays: Weekday[];
  quietEnabled: boolean;
  quietStart: string;
  quietEnd: string;
  catchUp: CatchUpPolicy;
  enabled: boolean;
}

function emptyDraft(): FormDraft {
  return {
    id: null,
    folderId: "",
    ruleId: "",
    scheduleType: "interval",
    intervalMins: 15,
    time: "09:00",
    weekdays: [],
    quietEnabled: false,
    quietStart: "22:00",
    quietEnd: "07:00",
    catchUp: "run_once",
    enabled: true,
  };
}

function draftFromSchedule(s: Schedule): FormDraft {
  return {
    id: s.id,
    folderId: s.folderId ?? "",
    ruleId: s.ruleId ?? "",
    scheduleType: s.scheduleType,
    intervalMins: Math.max(1, Math.round((s.config.intervalSecs ?? 900) / 60)),
    time: s.config.time ?? "09:00",
    weekdays: s.config.weekdays.length > 0 ? s.config.weekdays : [],
    quietEnabled: s.config.quietHours !== null,
    quietStart: s.config.quietHours?.start ?? "22:00",
    quietEnd: s.config.quietHours?.end ?? "07:00",
    catchUp: s.config.catchUp,
    enabled: s.enabled,
  };
}

function describeSchedule(s: Schedule): string {
  switch (s.scheduleType) {
    case "realtime":
      return "Real-time as files settle";
    case "interval": {
      const secs = s.config.intervalSecs ?? 900;
      const mins = Math.round(secs / 60);
      return mins >= 60 && mins % 60 === 0
        ? `Every ${plural(mins / 60, "hour")}`
        : `Every ${plural(mins, "minute")}`;
    }
    case "daily":
      return `Daily at ${s.config.time ?? "09:00"}`;
    case "weekly": {
      const days = s.config.weekdays
        .map((w) => WEEKDAY_OPTIONS.find((o) => o.value === w)?.label ?? w)
        .join(", ");
      return `Weekly (${days || "—"}) at ${s.config.time ?? "09:00"}`;
    }
    case "on_launch":
      return "On app launch";
    case "manual":
      return "Manual only";
  }
}

function formatNextRun(ms: number | null): string {
  if (ms === null) return "—";
  return new Date(ms).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function SchedulerPage() {
  const {
    schedules,
    lastOutcome,
    runningId,
    status,
    loadError,
    actionError,
    load,
    upsert,
    setEnabled,
    remove,
    runNow,
    clearActionError,
    clearLastOutcome,
  } = useSchedules();

  const [folders, setFolders] = useState<MonitoredFolder_Serialize[]>([]);
  const [rules, setRules] = useState<Rule_Serialize[]>([]);
  const [editorOpen, setEditorOpen] = useState(false);
  const [draft, setDraft] = useState<FormDraft>(emptyDraft);
  const [formError, setFormError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<Schedule | null>(null);

  useEffect(() => {
    void load();
    void commands.foldersList().then((res) => {
      if (res.status === "ok") setFolders(res.data);
    });
    void commands.rulesList().then((res) => {
      if (res.status === "ok") setRules(res.data);
    });
  }, [load]);

  const folderName = (id: string | null) =>
    id ? folders.find((f) => f.id === id)?.name ?? id : null;
  const ruleName = (id: string | null) =>
    id ? rules.find((r) => r.id === id)?.name ?? id : null;

  const scopeTitle = (s: Schedule) => {
    const f = folderName(s.folderId);
    const r = ruleName(s.ruleId);
    if (f && r) return `${f} · ${r}`;
    return f ?? r ?? "All monitored items";
  };

  const openNew = () => {
    setFormError(null);
    setDraft(emptyDraft());
    setEditorOpen(true);
  };

  const openEdit = (s: Schedule) => {
    setFormError(null);
    setDraft(draftFromSchedule(s));
    setEditorOpen(true);
  };

  const toggleWeekday = (day: Weekday) => {
    setDraft((prev) => {
      const exists = prev.weekdays.includes(day);
      const next = exists
        ? prev.weekdays.filter((d) => d !== day)
        : WEEKDAY_OPTIONS.map((w) => w.value).filter(
            (d) => d === day || prev.weekdays.includes(d),
          );
      return { ...prev, weekdays: next };
    });
  };

  const handleSave = async (e: FormEvent) => {
    e.preventDefault();
    setFormError(null);

    if (!draft.folderId && !draft.ruleId) {
      setFormError("Select a target folder, a target rule, or both.");
      return;
    }
    if (draft.scheduleType === "interval" && draft.intervalMins < 1) {
      setFormError("Interval must be at least 1 minute.");
      return;
    }
    if (
      (draft.scheduleType === "daily" || draft.scheduleType === "weekly") &&
      !HHMM_RE.test(draft.time.trim())
    ) {
      setFormError("Run time must use 24-hour HH:MM format (for example 09:30).");
      return;
    }
    if (draft.scheduleType === "weekly" && draft.weekdays.length === 0) {
      setFormError("Select at least one day of the week.");
      return;
    }
    if (
      draft.quietEnabled &&
      (!HHMM_RE.test(draft.quietStart.trim()) || !HHMM_RE.test(draft.quietEnd.trim()))
    ) {
      setFormError("Quiet hours must use 24-hour HH:MM format.");
      return;
    }

    const input: ScheduleInput = {
      id: draft.id,
      folderId: draft.folderId || null,
      ruleId: draft.ruleId || null,
      scheduleType: draft.scheduleType,
      enabled: draft.enabled,
      config: {
        intervalSecs:
          draft.scheduleType === "interval"
            ? Math.max(60, Math.round(draft.intervalMins * 60))
            : null,
        time:
          draft.scheduleType === "daily" || draft.scheduleType === "weekly"
            ? draft.time.trim()
            : null,
        weekdays: draft.scheduleType === "weekly" ? draft.weekdays : [],
        quietHours: draft.quietEnabled
          ? { start: draft.quietStart.trim(), end: draft.quietEnd.trim() }
          : null,
        catchUp: draft.catchUp,
      },
    };

    const saved = await upsert(input);
    if (saved) {
      setEditorOpen(false);
    }
  };

  let body;
  if (status === "error" && loadError) {
    body = <ErrorState error={loadError} onRetry={() => void load()} />;
  } else if (status !== "ready") {
    body = <LoadingState label="Loading schedules…" />;
  } else if (schedules.length === 0 && !editorOpen) {
    body = (
      <EmptyState
        icon={CalendarClock}
        title="No schedules configured"
        description="Rules without an explicit schedule run automatically in real time as files settle. Add a schedule to switch specific folders or rules to intervals, daily/weekly times, launch runs, or manual execution."
        action={
          <Button variant="primary" onClick={openNew}>
            <Plus aria-hidden />
            New schedule
          </Button>
        }
      />
    );
  } else if (schedules.length > 0) {
    body = (
      <div className="divide-y divide-border rounded-lg border border-border bg-surface">
        {schedules.map((s) => {
          const title = scopeTitle(s);
          return (
            <div
              key={s.id}
              className="flex flex-wrap items-center justify-between gap-4 p-4 hover:bg-hover"
            >
              <div className="min-w-0 flex-1 space-y-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-medium text-fg">{title}</span>
                  <span className="rounded-sm bg-muted px-2 py-0.5 text-xs font-medium text-fg-muted">
                    {TYPE_LABELS[s.scheduleType]}
                  </span>
                  {s.config.quietHours && (
                    <span className="inline-flex items-center gap-1 rounded-sm bg-info-subtle px-2 py-0.5 text-xs text-info">
                      <Clock className="size-3" aria-hidden />
                      Quiet {s.config.quietHours.start}–{s.config.quietHours.end}
                    </span>
                  )}
                </div>
                <p className="text-xs text-fg-muted">
                  {describeSchedule(s)} · Missed policy:{" "}
                  {s.config.catchUp === "run_once" ? "Run once" : "Skip"} · Last run:{" "}
                  {formatRelative(s.lastRunAt)} · Next: {formatNextRun(s.nextRunAt)}
                </p>
              </div>

              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={runningId === s.id}
                  onClick={() => void runNow(s.id)}
                >
                  <Play aria-hidden />
                  {runningId === s.id ? "Running…" : "Run now"}
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={`Edit schedule for ${title}`}
                  onClick={() => openEdit(s)}
                >
                  <Pencil aria-hidden />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={`Delete schedule for ${title}`}
                  onClick={() => setDeleting(s)}
                >
                  <Trash2 aria-hidden />
                </Button>
                <label htmlFor={`sched-toggle-${s.id}`} className="sr-only">
                  Toggle schedule {title}
                </label>
                <Switch
                  id={`sched-toggle-${s.id}`}
                  checked={s.enabled}
                  onChange={(next) => void setEnabled(s.id, next)}
                />
              </div>
            </div>
          );
        })}
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <PageHeader
        title="Scheduler"
        description="Configure when folders or rules run — real-time, periodic intervals, daily/weekly times, launch runs, or manual triggers."
        actions={
          <Button variant="primary" onClick={openNew}>
            <Plus aria-hidden />
            New schedule
          </Button>
        }
      />

      <div className="min-h-0 flex-1 overflow-y-auto p-page">
        <div className="mx-auto max-w-content space-y-4">
          {actionError && <ErrorState error={actionError} onDismiss={clearActionError} />}

          {lastOutcome && (
            <div
              role="status"
              className="flex items-center justify-between gap-2 rounded-lg border border-border bg-subtle p-3 text-sm text-fg"
            >
              <span>
                {OUTCOME_LABELS[lastOutcome.status]}
                {lastOutcome.report &&
                  ` · ${plural(lastOutcome.report.completed.length, "change")} completed`}
              </span>
              <Button size="sm" variant="ghost" onClick={clearLastOutcome}>
                Dismiss
              </Button>
            </div>
          )}

          {editorOpen && (
            <form
              onSubmit={(e) => void handleSave(e)}
              className="space-y-4 rounded-lg border border-border-strong bg-subtle p-4"
            >
              <h2 className="text-base font-semibold text-fg">
                {draft.id ? "Edit schedule" : "New schedule"}
              </h2>

              <div className="grid gap-3 sm:grid-cols-3">
                <div className="space-y-1">
                  <label htmlFor="sched-folder" className="block text-xs font-medium text-fg">
                    Target folder
                  </label>
                  <select
                    id="sched-folder"
                    value={draft.folderId}
                    onChange={(e) => setDraft({ ...draft, folderId: e.target.value })}
                    className="h-control w-full rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
                  >
                    <option value="">Any / none selected</option>
                    {folders.map((f) => (
                      <option key={f.id} value={f.id}>
                        {f.name}
                      </option>
                    ))}
                  </select>
                </div>

                <div className="space-y-1">
                  <label htmlFor="sched-rule" className="block text-xs font-medium text-fg">
                    Target rule
                  </label>
                  <select
                    id="sched-rule"
                    value={draft.ruleId}
                    onChange={(e) => setDraft({ ...draft, ruleId: e.target.value })}
                    className="h-control w-full rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
                  >
                    <option value="">All enabled rules</option>
                    {rules.map((r) => (
                      <option key={r.id} value={r.id}>
                        {r.name}
                      </option>
                    ))}
                  </select>
                </div>

                <div className="space-y-1">
                  <label htmlFor="sched-type" className="block text-xs font-medium text-fg">
                    Schedule type
                  </label>
                  <select
                    id="sched-type"
                    value={draft.scheduleType}
                    onChange={(e) =>
                      setDraft({ ...draft, scheduleType: e.target.value as ScheduleType })
                    }
                    className="h-control w-full rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
                  >
                    {(Object.keys(TYPE_LABELS) as ScheduleType[]).map((key) => (
                      <option key={key} value={key}>
                        {TYPE_LABELS[key]}
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              {draft.scheduleType === "interval" && (
                <div className="space-y-2">
                  <span className="block text-xs font-medium text-fg">Interval cadence</span>
                  <div className="flex flex-wrap items-center gap-2">
                    {INTERVAL_PRESETS.map((p) => {
                      const mins = p.secs / 60;
                      const active = draft.intervalMins === mins;
                      return (
                        <Button
                          key={p.secs}
                          size="sm"
                          variant={active ? "primary" : "secondary"}
                          onClick={() => setDraft({ ...draft, intervalMins: mins })}
                        >
                          {p.label}
                        </Button>
                      );
                    })}
                    <label className="inline-flex items-center gap-1.5 text-xs text-fg-muted">
                      <span>Custom (min):</span>
                      <input
                        type="number"
                        min={1}
                        aria-label="Interval minutes"
                        value={draft.intervalMins}
                        onChange={(e) =>
                          setDraft({
                            ...draft,
                            intervalMins: Math.max(1, Number(e.target.value) || 1),
                          })
                        }
                        className="h-control-sm w-20 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
                      />
                    </label>
                  </div>
                </div>
              )}

              {(draft.scheduleType === "daily" || draft.scheduleType === "weekly") && (
                <div className="grid gap-3 sm:grid-cols-2">
                  <div className="space-y-1">
                    <label htmlFor="sched-time" className="block text-xs font-medium text-fg">
                      Run time
                    </label>
                    <input
                      id="sched-time"
                      type="text"
                      placeholder="09:00"
                      value={draft.time}
                      onChange={(e) => setDraft({ ...draft, time: e.target.value })}
                      className="h-control w-36 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
                    />
                  </div>

                  {draft.scheduleType === "weekly" && (
                    <div className="space-y-1">
                      <span className="block text-xs font-medium text-fg">Days of week</span>
                      <div className="flex flex-wrap gap-1">
                        {WEEKDAY_OPTIONS.map((w) => {
                          const selected = draft.weekdays.includes(w.value);
                          return (
                            <Button
                              key={w.value}
                              size="sm"
                              variant={selected ? "primary" : "secondary"}
                              aria-pressed={selected}
                              onClick={() => toggleWeekday(w.value)}
                            >
                              {w.label}
                            </Button>
                          );
                        })}
                      </div>
                    </div>
                  )}
                </div>
              )}

              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-2">
                  <label className="inline-flex cursor-pointer items-center gap-2 text-sm text-fg">
                    <input
                      type="checkbox"
                      checked={draft.quietEnabled}
                      onChange={(e) =>
                        setDraft({ ...draft, quietEnabled: e.target.checked })
                      }
                    />
                    Enable quiet hours
                  </label>
                  {draft.quietEnabled && (
                    <div className="flex items-center gap-2">
                      <input
                        type="text"
                        aria-label="Quiet start"
                        value={draft.quietStart}
                        onChange={(e) => setDraft({ ...draft, quietStart: e.target.value })}
                        className="h-control-sm w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
                      />
                      <span className="text-xs text-fg-muted">to</span>
                      <input
                        type="text"
                        aria-label="Quiet end"
                        value={draft.quietEnd}
                        onChange={(e) => setDraft({ ...draft, quietEnd: e.target.value })}
                        className="h-control-sm w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
                      />
                    </div>
                  )}
                </div>

                <div className="space-y-1">
                  <label htmlFor="sched-catchup" className="block text-xs font-medium text-fg">
                    Missed run policy
                  </label>
                  <select
                    id="sched-catchup"
                    value={draft.catchUp}
                    onChange={(e) =>
                      setDraft({ ...draft, catchUp: e.target.value as CatchUpPolicy })
                    }
                    className={cn(
                      "h-control w-full rounded-md border border-border-strong bg-surface px-2 text-sm text-fg",
                    )}
                  >
                    <option value="run_once">Run once on wake</option>
                    <option value="skip">Skip missed runs</option>
                  </select>
                </div>
              </div>

              {formError && (
                <p role="alert" className="text-xs text-danger">
                  {formError}
                </p>
              )}

              <div className="flex justify-end gap-2">
                <Button variant="ghost" onClick={() => setEditorOpen(false)}>
                  Cancel
                </Button>
                <Button type="submit" variant="primary">
                  Save schedule
                </Button>
              </div>
            </form>
          )}

          {body}
        </div>
      </div>

      <ConfirmDialog
        open={deleting !== null}
        title="Delete schedule?"
        confirmLabel="Delete"
        danger
        onConfirm={() => {
          if (deleting) void remove(deleting.id);
          setDeleting(null);
        }}
        onCancel={() => setDeleting(null)}
      >
        <p>
          Deleting this schedule reverts the target folder or rule to default real-time
          monitoring when enabled. No files will be moved or deleted.
        </p>
      </ConfirmDialog>
    </div>
  );
}
