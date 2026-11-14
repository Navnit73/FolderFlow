import { BellRing, CheckCheck, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { events } from "@/bindings";
import { ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { formatRelative } from "@/lib/format";
import { notificationsService, type NotificationRecord } from "@/services/configIo";
import { toUiError } from "@/services/result";
import { useSettings } from "@/stores/settings";
import type { NotificationEventPrefs, UiError } from "@/types/settings";
import { SettingRow } from "./SettingRow";

const PER_TYPE_LABELS: { key: keyof NotificationEventPrefs; label: string; desc: string }[] = [
  { key: "successSummary", label: "Organization summary", desc: "Periodic summary of organized files." },
  { key: "largeBatchDone", label: "Large batch completion", desc: "Notify when a bulk run crosses the threshold." },
  { key: "failure", label: "Operation errors", desc: "Alert if moves, copies, or renames fail." },
  { key: "permissionIssue", label: "Permission issues", desc: "Alert if macOS denies folder access." },
  { key: "destinationUnavailable", label: "Destination unavailable", desc: "Alert when a target folder or volume is missing." },
  { key: "duplicatesFound", label: "Duplicates discovered", desc: "Notify after duplicate scans find reclaimable space." },
  { key: "monitoringStopped", label: "Monitoring interrupted", desc: "Alert when a folder watcher stops unexpectedly." },
];

export function NotificationsSection() {
  const settings = useSettings((s) => s.settings);
  const saveError = useSettings((s) => s.saveError);
  const update = useSettings((s) => s.update);
  const [recent, setRecent] = useState<NotificationRecord[]>([]);
  const [testNote, setTestNote] = useState<string | null>(null);
  const [notifError, setNotifError] = useState<UiError | null>(null);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    void notificationsService
      .list()
      .then((rows) => {
        if (!active) return;
        setRecent((cur) => {
          const seen = new Set(cur.map((r) => r.id));
          return [...cur, ...rows.filter((r) => !seen.has(r.id))].slice(0, 100);
        });
      })
      .catch(() => {
        // Ignore transient load error in sub-panel.
      });
    void events.appNotificationEvent
      .listen((ev) => {
        if (!active) return;
        setRecent((cur) => [ev.payload, ...cur.filter((r) => r.id !== ev.payload.id)].slice(0, 100));
      })
      .then((fn) => {
        if (active) {
          unlisten = fn;
        } else {
          fn();
        }
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  if (!settings) return null;
  const n = settings.notifications;
  const masterEnabled = settings.general.showNotifications && n.enabled;

  const patch = (next: Partial<typeof n>) =>
    void update((cur) => ({ ...cur, notifications: { ...cur.notifications, ...next } }));

  const patchPerType = (key: keyof NotificationEventPrefs, val: boolean) =>
    patch({ perType: { ...n.perType, [key]: val } });

  const handleTest = async () => {
    setNotifError(null);
    try {
      const sent = await notificationsService.sendTest();
      setTestNote(
        sent ? `Delivered test alert: “${sent.title}”` : "Notifications are disabled globally.",
      );
      setRecent(await notificationsService.list());
    } catch (e) {
      setNotifError(toUiError(e));
    }
  };

  const handleMarkAllRead = async () => {
    setNotifError(null);
    try {
      await notificationsService.markRead(null);
      setRecent((cur) => cur.map((r) => ({ ...r, read: true })));
    } catch (e) {
      setNotifError(toUiError(e));
    }
  };

  const handleClear = async () => {
    setNotifError(null);
    try {
      await notificationsService.clear();
      setRecent([]);
    } catch (e) {
      setNotifError(toUiError(e));
    }
  };

  return (
    <div className="space-y-6">
      <div>
        {saveError && (
          <p role="alert" className="mb-3 text-sm text-danger">
            {saveError.message}
          </p>
        )}
        <SettingRow
          label="Enable notifications"
          description="Master toggle for all desktop summary and alert notifications."
        >
          <Switch
            id="notif-master"
            checked={masterEnabled}
            onChange={(checked) =>
              void update((cur) => ({
                ...cur,
                general: { ...cur.general, showNotifications: checked },
                notifications: { ...cur.notifications, enabled: checked },
              }))
            }
          />
        </SettingRow>
        <SettingRow
          label="Batching window (seconds)"
          description="Coalesce bursts within this window into one summary (never 1 notification per file)."
        >
          <input
            aria-label="Batching window (seconds)"
            type="number"
            min={0}
            max={300}
            value={n.batchWindowSecs}
            onChange={(e) => patch({ batchWindowSecs: Math.max(0, Number(e.target.value) || 0) })}
            className="h-control w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          />
        </SettingRow>
        <SettingRow
          label="Cooldown between same-type alerts (seconds)"
          description="Suppresses rapid repeated notifications of the same event type (0–3600s)."
        >
          <input
            aria-label="Cooldown (seconds)"
            type="number"
            min={0}
            max={3600}
            value={n.cooldownSecs}
            onChange={(e) => patch({ cooldownSecs: Math.max(0, Number(e.target.value) || 0) })}
            className="h-control w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          />
        </SettingRow>
        <SettingRow
          label="Large batch threshold (files)"
          description="Emit a dedicated completion alert once a batch organizes this many files (2–10000)."
        >
          <input
            aria-label="Large batch threshold"
            type="number"
            min={2}
            max={10000}
            value={n.largeBatchThreshold}
            onChange={(e) => patch({ largeBatchThreshold: Math.max(2, Number(e.target.value) || 2) })}
            className="h-control w-24 rounded-md border border-border-strong bg-surface px-2 text-sm text-fg"
          />
        </SettingRow>
      </div>

      <section>
        <h3 className="text-sm font-semibold text-fg">Per-event preferences</h3>
        <div className="mt-1">
          {PER_TYPE_LABELS.map((item) => (
            <SettingRow key={item.key} label={item.label} description={item.desc}>
              <Switch
                id={`notif-type-${item.key}`}
                checked={n.perType[item.key]}
                disabled={!masterEnabled}
                onChange={(checked) => patchPerType(item.key, checked)}
              />
            </SettingRow>
          ))}
        </div>
      </section>

      <section className="space-y-3 rounded-xl border border-border bg-subtle p-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <h3 className="text-sm font-semibold text-fg">Recent notifications</h3>
            <p className="text-xs text-fg-muted">
              Latest coalesced notifications delivered during this session.
            </p>
          </div>
          <div className="flex gap-2">
            <Button size="sm" variant="secondary" onClick={() => void handleTest()}>
              <BellRing aria-hidden />
              Send test
            </Button>
            {recent.length > 0 && (
              <>
                <Button size="sm" variant="secondary" onClick={() => void handleMarkAllRead()}>
                  <CheckCheck aria-hidden />
                  Mark all read
                </Button>
                <Button size="sm" variant="ghost" onClick={() => void handleClear()}>
                  <Trash2 aria-hidden />
                  Clear
                </Button>
              </>
            )}
          </div>
        </div>
        {notifError && <ErrorState error={notifError} onDismiss={() => setNotifError(null)} />}
        {testNote && (
          <p role="status" className="text-xs text-fg-muted">
            {testNote}
          </p>
        )}
        {recent.length === 0 ? (
          <p className="text-xs text-fg-subtle">No notifications emitted yet.</p>
        ) : (
          <ul className="divide-y divide-border rounded-lg border border-border bg-surface text-xs">
            {recent.slice(0, 8).map((rec) => (
              <li key={rec.id} className="flex items-start justify-between gap-3 px-3 py-2">
                <div>
                  <p className="font-medium text-fg">{rec.title}</p>
                  <p className="text-fg-muted">{rec.body}</p>
                </div>
                <span className="shrink-0 text-fg-subtle">{formatRelative(rec.createdAtMs)}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
